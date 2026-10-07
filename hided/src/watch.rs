//! The watch behind the Explorer's live refresh (PRD B2, D-04, D-08, S5.5 B5,
//! B43, core-host-node D-21).
//!
//! An Explorer's folders are its node's: the checkout root and the folders
//! the core reports as expanded, most recently expanded last. The node that
//! holds them stamps them (`Call::Stamps`, `hide_host::list::stamps`): which
//! directory each path names and when its list of names last changed. A
//! watcher asks for those stamps on a poll and announces a folder whose stamp
//! moved as a `directory_changed` frame naming the node, which makes the
//! client re-read that one folder; a folder past the cap is not watched, and
//! the client draws the refresh badge on its row instead.
//!
//! There are two watchers, both addressed by node id: this machine's
//! Explorer, and the selected device's while its Explorer shows on a ready
//! helper. The core's own node answers in this process, so its folders are
//! polled often and a burst is coalesced; another node's answer crosses a
//! link, so it is polled less often and announced at once.
//!
//! The resource is bounded on purpose (engineering 14 and 15): at most
//! `WATCH_CAP` folders are stamped per poll, one request per poll carries
//! every folder, the next poll does not start until the last has answered,
//! and each watcher is one task that ends with the daemon; a node that fails
//! is asked again at the linked interval, not the in-process one. The same
//! cap is why the web's own listing cache holds the same number: the two
//! sides agree on which folders are live without a second protocol.
//!
//! The root is stamped by one identity, so a root replaced after it was
//! pinned is refused rather than followed. This machine's root is pinned by
//! the identity the boundary verified when the checkout was registered, the
//! one every listing of it is checked against, so the watcher never opens the
//! path on its own and cannot watch a folder the Explorer would refuse to
//! list. A device's root is pinned when its node first opens it, under that
//! node's own confinement.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hide_node_link::protocol::{Call, RootOpened, RootRef};
use hide_node_link::{ErrorCode, LinkError, NodeLink, RootIdentity, call_as};
use serde_json::Value;
use tokio::sync::{Notify, broadcast};

use crate::core::CoreHandle;

/// Folders watched at once, the checkout root included. The web's listing
/// cache uses the same number (`web/src/watch.ts`).
pub const WATCH_CAP: usize = 64;

/// One folder's changes within this window collapse into a single frame, so a
/// burst (a git checkout, an editor's atomic save) is one client re-read.
const COALESCE: Duration = Duration::from_millis(200);
/// How often a node answering in this process is asked.
const IN_PROCESS_POLL: Duration = Duration::from_millis(200);
/// How often a node across a link is asked; its stamps are announced at once.
const LINKED_POLL: Duration = Duration::from_secs(2);
const STAMPS_TIMEOUT: Duration = Duration::from_secs(5);

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

/// The folders to stamp on one node: the checkout root and its watched
/// folders as absolute paths on that node, root first, and the identity the
/// root is stamped by when it is already known.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub node: String,
    pub root: String,
    pub folders: Vec<String>,
    pub pinned: Option<RootIdentity>,
}

impl Target {
    /// `root`'s Explorer on `node` with `expanded` open.
    pub fn of(node: &str, root: String, expanded: &[String]) -> Self {
        let folders = watched_folders(&root, expanded)
            .into_iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        Self {
            node: node.to_owned(),
            root,
            folders,
            pinned: None,
        }
    }

    /// The same target stamped by `identity`, the root's verified identity.
    pub fn pinned_by(self, identity: RootIdentity) -> Self {
        Self {
            pinned: Some(identity),
            ..self
        }
    }
}

/// What a snapshot asks the device watch to stamp, or `None` when no device
/// Explorer is showing on a ready helper. `node` is the core's own machine,
/// whose Explorer the other watcher covers.
pub fn device_target(value: &Value, node: &str) -> Option<Target> {
    let device = value
        .pointer("/rest/navigator/focused_device_id")
        .and_then(Value::as_str)
        .filter(|device| *device != node)?;
    let ui = value.pointer("/rest/ui_state")?;
    if ui.get("right_panel_visible").and_then(Value::as_bool) != Some(true)
        || ui.get("right_panel_section").and_then(Value::as_str) != Some("explorer")
    {
        return None;
    }
    let ready = value
        .pointer("/rest/navigator/devices")
        .and_then(Value::as_array)?
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(device))?
        .pointer("/host/state")
        .and_then(Value::as_str)
        == Some("ready");
    if !ready {
        return None;
    }
    let session = value
        .pointer("/rest/status/remote")
        .and_then(Value::as_array)?
        .iter()
        .find(|remote| remote.get("target_id").and_then(Value::as_str) == Some(device))?
        .get("session")?;
    let checkout_id = session.get("focused_checkout_id").and_then(Value::as_str)?;
    let root = session
        .get("workspaces")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|workspace| workspace.get("checkouts").and_then(Value::as_array))
        .flatten()
        .find(|checkout| checkout.get("id").and_then(Value::as_str) == Some(checkout_id))?
        .get("path")
        .and_then(Value::as_str)?
        .to_owned();
    let expanded: Vec<String> = ui
        .pointer(&format!("/device_expanded_paths/{}", pointer_token(device)))
        .and_then(Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some(Target::of(device, root, &expanded))
}

fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// The daemon's watch service: this machine's Explorer, the selected
/// device's, and the broadcast every client subscribes to.
pub struct WatchService {
    frames: broadcast::Sender<String>,
    local: Watcher,
    device: Watcher,
}

impl WatchService {
    pub fn new(core: Arc<CoreHandle>) -> Self {
        let (frames, _) = broadcast::channel(64);
        let local = Watcher::spawn("local", Arc::clone(&core), frames.clone());
        let device = Watcher::spawn("device", core, frames.clone());
        Self {
            frames,
            local,
            device,
        }
    }

    /// Announces this machine's Explorer folders to stamp, or none; a repeat
    /// of the current target is free.
    pub fn reconcile(&self, target: Option<Target>) {
        self.local.set(target);
    }

    /// Announces the device Explorer to stamp, or none.
    pub fn reconcile_device(&self, target: Option<Target>) {
        self.device.set(target);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.frames.subscribe()
    }
}

struct Watcher {
    kind: &'static str,
    target: Arc<Mutex<Option<Target>>>,
    /// Wakes the poll when the target changes, so a folder the page has just
    /// listed is stamped at once rather than up to one interval later.
    changed: Arc<Notify>,
}

impl Watcher {
    fn spawn(kind: &'static str, core: Arc<CoreHandle>, frames: broadcast::Sender<String>) -> Self {
        let target = Arc::new(Mutex::new(None));
        let changed = Arc::new(Notify::new());
        tokio::spawn(run(
            kind,
            core,
            frames,
            Arc::clone(&target),
            Arc::clone(&changed),
        ));
        Self {
            kind,
            target,
            changed,
        }
    }

    fn set(&self, target: Option<Target>) {
        let mut current = self.target.lock().expect("watch target");
        if *current == target {
            return;
        }
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "hided", "kind": "watch.target", "watch": self.kind,
                "node": target.as_ref().map(|target| &target.node),
                "folders": target.as_ref().map_or(0, |target| target.folders.len()),
            })
        );
        *current = target;
        self.changed.notify_one();
    }
}

/// Each watched folder's last stamp, `None` for a folder the node could not
/// read, keyed by its absolute path on the node.
type Stamps = HashMap<String, Option<String>>;

/// One node's answer for a target: the root identity it was stamped by,
/// whether the node answers in this process, and a stamp per folder.
struct Answer {
    identity: RootIdentity,
    in_process: bool,
    stamps: Vec<Option<String>>,
}

/// What one watcher remembers between polls.
#[derive(Default)]
struct Watching {
    /// The node and root the stamps belong to, and the root's identity when
    /// it was first opened for them.
    pinned: Option<((String, String), RootIdentity)>,
    /// The node refused the pinned identity because the root was replaced;
    /// the next poll opens it again.
    replaced: bool,
    stamps: Stamps,
    /// Folders whose stamp moved, and when it last moved.
    moving: HashMap<String, Instant>,
}

impl Watching {
    /// The root identity to stamp `target` by: the one it carries, else the
    /// one pinned for its node and root.
    fn pin_for(&self, target: &Target) -> Option<RootIdentity> {
        target.pinned.or_else(|| {
            self.pinned
                .as_ref()
                .filter(|(key, _)| !self.replaced && key.0 == target.node && key.1 == target.root)
                .map(|(_, identity)| *identity)
        })
    }

    /// Takes one stamping of `target` and answers the folders to announce. A
    /// folder whose stamp moved is announced once it has held still for
    /// `coalesce`, at once when that is zero; a folder watched for the first
    /// time in the same checkout counts as moved, because the page listed it
    /// when it was expanded, which may be before its first stamp. A new
    /// checkout starts over and announces nothing; the same checkout stamped
    /// by another root identity is a replaced root opened again, so every
    /// folder is announced for the page to re-read.
    fn observe(
        &mut self,
        target: &Target,
        identity: RootIdentity,
        now: Stamps,
        at: Instant,
        coalesce: Duration,
    ) -> Vec<String> {
        let key = (target.node.clone(), target.root.clone());
        self.replaced = false;
        match &self.pinned {
            Some((pinned, before)) if *pinned == key && *before == identity => {}
            previous => {
                let mut announced = Vec::new();
                if previous.as_ref().is_some_and(|(pinned, _)| *pinned == key) {
                    announced = now.keys().cloned().collect();
                    announced.sort();
                }
                self.pinned = Some((key, identity));
                self.stamps = now;
                self.moving.clear();
                return announced;
            }
        }
        let moved = changed_folders(&self.stamps, &now);
        self.stamps = now;
        self.moving.retain(|path, _| self.stamps.contains_key(path));
        let mut announced = Vec::new();
        if coalesce.is_zero() {
            announced = moved;
        } else {
            for path in &moved {
                self.moving.insert(path.clone(), at);
            }
            self.moving.retain(|path, since| {
                let settled = !moved.contains(path) && at.duration_since(*since) >= coalesce;
                if settled {
                    announced.push(path.clone());
                }
                !settled
            });
            announced.sort();
        }
        announced
    }
}

/// The folders whose stamp differs from the one recorded before, and a folder
/// watched for the first time.
fn changed_folders(before: &Stamps, now: &Stamps) -> Vec<String> {
    let mut changed: Vec<String> = now
        .iter()
        .filter(|(path, stamp)| before.get(*path).is_none_or(|previous| previous != *stamp))
        .map(|(path, _)| path.clone())
        .collect();
    changed.sort();
    changed
}

/// A folder's path relative to the checkout root, as the node names it.
fn relative(root: &str, folder: &str) -> Option<String> {
    hide_platform::path::wire_relative(root, folder)
        .ok()
        .map(hide_platform::path::RelPath::into_string)
}

fn frame(node: &str, path: &str) -> String {
    serde_json::json!({
        "type": "directory_changed",
        "payload": {"path": path, "device_id": node},
    })
    .to_string()
}

/// Asks `link` to stamp `folders` (relative to `root`), opening the root
/// first when no identity is pinned for it. A root replaced since it was
/// pinned is refused by the node, never followed.
fn stamp_on(
    link: &dyn NodeLink,
    root: &str,
    pinned: Option<RootIdentity>,
    folders: &[String],
) -> Result<(RootIdentity, Vec<Option<String>>), LinkError> {
    let identity = match pinned {
        Some(identity) => identity,
        None => {
            call_as::<RootOpened>(
                link,
                Call::RootOpen {
                    root: root.to_owned(),
                },
                STAMPS_TIMEOUT,
            )?
            .identity
        }
    };
    let stamps: Vec<Option<String>> = call_as(
        link,
        Call::Stamps {
            root: RootRef {
                path: root.to_owned(),
                identity,
            },
            folders: folders.to_vec(),
        },
        STAMPS_TIMEOUT,
    )?;
    if stamps.len() != folders.len() {
        return Err(LinkError::Unknown(format!(
            "the node stamped {} folders of {}",
            stamps.len(),
            folders.len()
        )));
    }
    Ok((identity, stamps))
}

async fn run(
    kind: &'static str,
    core: Arc<CoreHandle>,
    frames: broadcast::Sender<String>,
    target: Arc<Mutex<Option<Target>>>,
    target_changed: Arc<Notify>,
) {
    let mut watching = Watching::default();
    let mut poll = IN_PROCESS_POLL;
    let mut failing: Option<String> = None;
    // The last node asked and its link, kept while it answers so a poll does
    // not cross to the core's owner thread to find it again.
    let mut linked: Option<(String, Arc<dyn NodeLink>)> = None;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(poll) => {}
            _ = target_changed.notified() => {}
        }
        let Some(current) = target.lock().expect("watch target").clone() else {
            watching = Watching::default();
            failing = None;
            linked = None;
            continue;
        };
        let pairs: Vec<(String, String)> = current
            .folders
            .iter()
            .filter_map(|folder| {
                relative(&current.root, folder).map(|relative| (folder.clone(), relative))
            })
            .collect();
        let relatives: Vec<String> = pairs.iter().map(|(_, relative)| relative.clone()).collect();
        let asked = current.clone();
        let pinned = watching.pin_for(&current);
        let known = linked
            .as_ref()
            .filter(|(node, _)| *node == current.node)
            .map(|(_, link)| Arc::clone(link));
        let core = Arc::clone(&core);
        let answer = tokio::task::spawn_blocking(move || {
            let link = match known {
                Some(link) => link,
                None => core
                    .node_link(&asked.node)
                    .map_err(|message| (message, false))?,
            };
            let (identity, stamps) = stamp_on(link.as_ref(), &asked.root, pinned, &relatives)
                .map_err(|error| {
                    let replaced = matches!(
                        &error,
                        LinkError::Refused(refusal) if refusal.code == ErrorCode::RootReplaced
                    );
                    (error.to_string(), replaced)
                })?;
            Ok((
                Arc::clone(&link),
                Answer {
                    identity,
                    in_process: link.in_process(),
                    stamps,
                },
            ))
        })
        .await
        .unwrap_or_else(|error| Err((format!("the watch worker ended: {error}"), false)));
        // A target that moved while the node answered is not what the answer
        // was for.
        if target.lock().expect("watch target").as_ref() != Some(&current) {
            continue;
        }
        let answer = match answer {
            Ok((link, answer)) => {
                linked = Some((current.node.clone(), link));
                answer
            }
            Err((message, replaced)) => {
                linked = None;
                poll = LINKED_POLL;
                // A device root its node opened is pinned here, so a root
                // replaced since is opened again through that node, which
                // confines it; this machine's root stays pinned by the
                // boundary, which refuses its listings the same way.
                if replaced && current.pinned.is_none() {
                    watching.replaced = true;
                }
                if failing.as_deref() != Some(message.as_str()) {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "component": "hided", "kind": "watch.failed", "watch": kind,
                            "node": current.node, "message": message,
                        })
                    );
                    failing = Some(message);
                }
                continue;
            }
        };
        failing = None;
        let coalesce = if answer.in_process {
            poll = IN_PROCESS_POLL;
            COALESCE
        } else {
            poll = LINKED_POLL;
            Duration::ZERO
        };
        let now: Stamps = pairs
            .into_iter()
            .map(|(folder, _)| folder)
            .zip(answer.stamps)
            .collect();
        for path in watching.observe(&current, answer.identity, now, Instant::now(), coalesce) {
            let _ = frames.send(frame(&current.node, &path));
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

    fn snapshot(section: &str, host_state: &str) -> Value {
        serde_json::json!({"rest": {
            "navigator": {
                "focused_device_id": "mac",
                "devices": [{"id": "local"}, {"id": "mac", "host": {"state": host_state}}],
            },
            "ui_state": {
                "right_panel_visible": true,
                "right_panel_section": section,
                "expanded_paths": ["/here/src"],
                "device_expanded_paths": {"mac": ["/r/src", "/elsewhere/x", "/r/src/deep"]},
            },
            "status": {"remote": [{"target_id": "mac", "session": {
                "focused_checkout_id": "remote:mac:checkout:w1",
                "workspaces": [{"checkouts": [{"id": "remote:mac:checkout:w1", "path": "/r"}]}],
            }}]},
        }})
    }

    /// Only a showing device Explorer on a ready helper is watched, and only
    /// that device's folders under its front checkout.
    #[test]
    fn a_device_explorer_is_watched_only_while_it_shows_on_a_ready_helper() {
        assert_eq!(
            device_target(&snapshot("explorer", "ready"), "local"),
            Some(Target {
                node: "mac".to_owned(),
                root: "/r".to_owned(),
                folders: vec![
                    "/r".to_owned(),
                    "/r/src".to_owned(),
                    "/r/src/deep".to_owned()
                ],
                pinned: None,
            })
        );
        assert_eq!(device_target(&snapshot("changes", "ready"), "local"), None);
        assert_eq!(
            device_target(&snapshot("explorer", "connecting"), "local"),
            None
        );
        let mut local = snapshot("explorer", "ready");
        local["rest"]["navigator"]["focused_device_id"] = "local".into();
        assert_eq!(device_target(&local, "local"), None);
    }

    #[test]
    fn a_moved_stamp_or_a_newly_watched_folder_is_announced() {
        let before = HashMap::from([
            ("/r".to_owned(), Some("1:2:3".to_owned())),
            ("/r/src".to_owned(), Some("1:5:3".to_owned())),
        ]);
        let now = HashMap::from([
            ("/r".to_owned(), Some("1:2:3".to_owned())),
            ("/r/src".to_owned(), None),
            ("/r/new".to_owned(), Some("1:9:9".to_owned())),
        ]);
        assert_eq!(
            changed_folders(&before, &now),
            vec!["/r/new".to_owned(), "/r/src".to_owned()]
        );
        assert_eq!(relative("/r", "/r"), Some(String::new()));
        assert_eq!(relative("/r", "/r/src/deep"), Some("src/deep".to_owned()));
        assert_eq!(relative("/r", "/rx/src"), None);
    }

    fn stamps(values: &[(&str, &str)]) -> Stamps {
        values
            .iter()
            .map(|(path, stamp)| ((*path).to_owned(), Some((*stamp).to_owned())))
            .collect()
    }

    /// A burst in one folder is one frame once the folder holds still, and a
    /// node across a link is announced at the poll that saw the change.
    #[test]
    fn a_burst_is_announced_once_it_settles() {
        let target = Target::of("local", "/r".to_owned(), &[]);
        let identity = RootIdentity {
            device: 1,
            inode: 2,
        };
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut watching = Watching::default();
        assert!(
            watching
                .observe(&target, identity, stamps(&[("/r", "a")]), at(0), COALESCE)
                .is_empty(),
            "the first stamping only records"
        );
        assert!(
            watching
                .observe(&target, identity, stamps(&[("/r", "b")]), at(200), COALESCE)
                .is_empty()
        );
        assert!(
            watching
                .observe(&target, identity, stamps(&[("/r", "c")]), at(400), COALESCE)
                .is_empty(),
            "a folder still moving is not announced"
        );
        assert_eq!(
            watching.observe(&target, identity, stamps(&[("/r", "c")]), at(600), COALESCE),
            vec!["/r".to_owned()]
        );
        assert!(
            watching
                .observe(&target, identity, stamps(&[("/r", "c")]), at(800), COALESCE)
                .is_empty()
        );

        let mut linked = Watching::default();
        linked.observe(
            &target,
            identity,
            stamps(&[("/r", "a")]),
            at(0),
            Duration::ZERO,
        );
        assert_eq!(
            linked.observe(
                &target,
                identity,
                stamps(&[("/r", "b")]),
                at(2000),
                Duration::ZERO
            ),
            vec!["/r".to_owned()]
        );
    }

    /// The root is stamped by the identity it was opened with: a root whose
    /// path now names another folder is refused, so a change behind the new
    /// path is never announced as the watched checkout's.
    #[cfg(unix)]
    #[test]
    fn a_root_replaced_after_it_was_opened_is_refused_not_followed() {
        use std::os::unix::fs::symlink;
        let node = hide_node::Local::of_process();
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let root_path = root.to_string_lossy().into_owned();
        let folders = vec![String::new()];
        let (identity, before) = stamp_on(&node, &root_path, None, &folders).unwrap();
        assert!(before[0].is_some());

        std::fs::rename(&root, sandbox.path().join("moved")).unwrap();
        symlink(&outside, &root).unwrap();
        std::fs::write(outside.join("other.txt"), "outside").unwrap();
        assert!(
            stamp_on(&node, &root_path, Some(identity), &folders).is_err(),
            "the replaced root is refused"
        );
    }

    /// This machine's root is stamped by the identity the boundary verified
    /// at registration, so a root swapped for a link to an outside folder
    /// before the first poll is refused, never pinned as the checkout.
    #[cfg(unix)]
    #[test]
    fn a_root_swapped_before_the_first_poll_is_refused() {
        use std::os::unix::fs::symlink;
        let node = hide_node::Local::of_process();
        let sandbox = tempfile::tempdir().unwrap();
        let home = sandbox.path().join("home");
        let root = home.join("checkout");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        let boundary = crate::boundary::Boundary::new(&home).unwrap();
        let root = boundary.home().join("checkout");
        boundary.set_roots(vec![crate::boundary::Root {
            workspace_id: "w".to_owned(),
            checkout_id: "c".to_owned(),
            path: root.clone(),
        }]);
        let root_path = root.to_string_lossy().into_owned();
        let identity = boundary.root_identity(&root_path).unwrap();
        let target = Target::of("local", root_path.clone(), &[]).pinned_by(identity);

        std::fs::rename(&root, home.join("moved")).unwrap();
        symlink(&outside, &root).unwrap();
        let pinned = Watching::default().pin_for(&target);
        assert_eq!(pinned, Some(identity), "the watcher opens nothing itself");
        let refused = stamp_on(&node, &root_path, pinned, &[String::new()]).unwrap_err();
        assert!(
            matches!(&refused, LinkError::Refused(error) if error.code == ErrorCode::RootReplaced),
            "{refused}"
        );
    }

    /// A device root its node refused as replaced is opened again on the next
    /// poll, and every watched folder is announced so the page re-reads the
    /// new directory behind the same path.
    #[test]
    fn a_replaced_device_root_is_opened_again_and_announced() {
        let target = Target::of("mac", "/r".to_owned(), &["/r/src".to_owned()]);
        let first = RootIdentity {
            device: 1,
            inode: 2,
        };
        let second = RootIdentity {
            device: 1,
            inode: 3,
        };
        let at = Instant::now();
        let both = stamps(&[("/r", "a"), ("/r/src", "b")]);
        let mut watching = Watching::default();
        assert!(
            watching
                .observe(&target, first, both.clone(), at, Duration::ZERO)
                .is_empty()
        );
        assert_eq!(watching.pin_for(&target), Some(first));

        watching.replaced = true;
        assert_eq!(
            watching.pin_for(&target),
            None,
            "the next poll opens the root"
        );
        assert_eq!(
            watching.observe(&target, second, both, at, Duration::ZERO),
            vec!["/r".to_owned(), "/r/src".to_owned()]
        );
        assert_eq!(watching.pin_for(&target), Some(second));
    }

    /// An expanded folder replaced under a watched root has a new stamp, so
    /// the next poll announces it and the page re-reads its new contents.
    #[test]
    fn a_replaced_expanded_folder_is_announced() {
        let node = hide_node::Local::of_process();
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let expanded = root.join("expanded");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&expanded).unwrap();
        let root_path = hide_platform::path::to_wire_lossy(&root);
        let target = Target::of(
            "local",
            root_path.clone(),
            &[hide_platform::path::to_wire_lossy(&expanded)],
        );
        let folders: Vec<String> = target
            .folders
            .iter()
            .map(|folder| relative(&root_path, folder).unwrap())
            .collect();
        let read = |pinned| {
            let (identity, answer) = stamp_on(&node, &root_path, pinned, &folders).unwrap();
            let now: Stamps = target.folders.iter().cloned().zip(answer).collect();
            (identity, now)
        };
        let mut watching = Watching::default();
        let (identity, now) = read(None);
        let start = Instant::now();
        assert!(
            watching
                .observe(&target, identity, now, start, Duration::ZERO)
                .is_empty()
        );

        std::fs::rename(&expanded, root.join("old-expanded")).unwrap();
        std::fs::create_dir(&expanded).unwrap();
        let (_, now) = read(Some(identity));
        let announced = watching.observe(&target, identity, now, start, Duration::ZERO);
        assert!(
            announced.contains(&target.folders[1]),
            "the replacement invalidates its cached listing: {announced:?}"
        );
    }
}
