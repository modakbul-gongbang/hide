//! The watch behind a device Explorer's live refresh (PRD S5.5 B5, B43).
//!
//! This machine's folders are polled from opened handles (`watch.rs`); a
//! device's folders are the device's, so its helper stamps them
//! (`hide_host::list::stamps`) and this task compares each stamp with the one
//! before. A changed folder is announced as the same `directory_changed` frame,
//! naming the device, and the client re-reads that one folder.
//!
//! The target is the selected device's front checkout while its Explorer is
//! showing and its helper is ready: its root and the most recently expanded
//! folders under it, at most `WATCH_CAP` in total by the same rule as this
//! machine's. Nothing is polled for a closed Explorer, another device, or a
//! helper that is not ready, so a watch never starts a helper connection and
//! ends with the surface that asked for it. One request per poll carries every
//! folder, and the next poll does not start until the last one has answered.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::broadcast;

use crate::core::CoreHandle;

/// How often a device's watched folders are stamped: one helper request per
/// interval for the whole watch set.
const DEVICE_POLL: Duration = Duration::from_secs(2);

/// Each watched folder's last stamp, `None` for a folder the helper could not
/// read, keyed by its absolute path on the device.
type Stamps = HashMap<String, Option<String>>;

/// The folders to stamp on one device: the checkout root and its watched
/// folders as absolute paths on that device, root first.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceTarget {
    pub device_id: String,
    pub root: String,
    pub folders: Vec<String>,
}

pub struct DeviceWatch {
    target: Arc<Mutex<Option<DeviceTarget>>>,
    /// Wakes the poll when the target changes, so a folder the page has just
    /// listed is stamped at once rather than up to one interval later.
    changed: Arc<tokio::sync::Notify>,
}

impl DeviceWatch {
    pub fn spawn(core: Arc<CoreHandle>, frames: broadcast::Sender<String>) -> Self {
        let target = Arc::new(Mutex::new(None));
        let changed = Arc::new(tokio::sync::Notify::new());
        tokio::spawn(run(core, frames, Arc::clone(&target), Arc::clone(&changed)));
        Self { target, changed }
    }

    pub fn set_target(&self, target: Option<DeviceTarget>) {
        let mut current = self.target.lock().expect("device watch target");
        if *current == target {
            return;
        }
        eprintln!(
            "{}",
            serde_json::json!({
                "component": "hided", "kind": "device_watch.target",
                "device": target.as_ref().map(|target| &target.device_id),
                "root": target.as_ref().map(|target| &target.root),
                "folders": target.as_ref().map_or(0, |target| target.folders.len()),
            })
        );
        *current = target;
        self.changed.notify_one();
    }
}

/// What a snapshot asks the device watch to stamp, or `None` when no device
/// Explorer is showing on a ready helper.
pub fn target_from_value(value: &Value) -> Option<DeviceTarget> {
    let device = value
        .pointer("/rest/navigator/focused_device_id")
        .and_then(Value::as_str)
        .filter(|device| *device != herdr_core::workspace::LOCAL_DEVICE_ID)?;
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
    let folders = crate::watch::watched_folders(&root, &expanded)
        .into_iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    Some(DeviceTarget {
        device_id: device.to_owned(),
        root,
        folders,
    })
}

fn pointer_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// A folder's path relative to the checkout root, as the helper names it.
fn relative(root: &str, folder: &str) -> Option<String> {
    if folder == root {
        return Some(String::new());
    }
    folder
        .strip_prefix(root.trim_end_matches('/'))
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_owned)
}

fn frame(device_id: &str, path: &str) -> String {
    serde_json::json!({
        "type": "directory_changed",
        "payload": {"path": path, "device_id": device_id},
    })
    .to_string()
}

/// The folders to announce: those whose stamp differs from the one recorded
/// before, and a folder watched for the first time in the same checkout. The
/// page listed that folder when it was expanded, which may be before its
/// first stamp, so one re-read closes the gap between the two.
fn changed_folders(before: &Stamps, now: &Stamps) -> Vec<String> {
    let mut changed: Vec<String> = now
        .iter()
        .filter(|(path, stamp)| before.get(*path).is_none_or(|previous| previous != *stamp))
        .map(|(path, _)| path.clone())
        .collect();
    changed.sort();
    changed
}

async fn run(
    core: Arc<CoreHandle>,
    frames: broadcast::Sender<String>,
    target: Arc<Mutex<Option<DeviceTarget>>>,
    target_changed: Arc<tokio::sync::Notify>,
) {
    let mut interval = tokio::time::interval(DEVICE_POLL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The device and root the recorded stamps belong to.
    let mut recorded: Option<((String, String), Stamps)> = None;
    let mut failing: Option<String> = None;
    loop {
        tokio::select! {
            _ = interval.tick() => {}
            _ = target_changed.notified() => {}
        }
        let Some(current) = target.lock().expect("device watch target").clone() else {
            recorded = None;
            continue;
        };
        let pairs: Vec<(String, String)> = current
            .folders
            .iter()
            .filter_map(|folder| {
                relative(&current.root, folder).map(|relative| (folder.clone(), relative))
            })
            .collect();
        let asked = current.clone();
        let relatives: Vec<String> = pairs.iter().map(|(_, relative)| relative.clone()).collect();
        let core = Arc::clone(&core);
        let answer = tokio::task::spawn_blocking(move || {
            let channel = core.device_channel(&asked.device_id)?;
            herdr_core::host_access::folder_stamps(channel.as_ref(), &asked.root, &relatives)
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|error| Err(format!("the device watch worker ended: {error}")));
        // A target that moved while the helper answered is not what the
        // answer was for.
        if target.lock().expect("device watch target").as_ref() != Some(&current) {
            continue;
        }
        let answer = answer.and_then(|stamps| {
            if stamps.len() == pairs.len() {
                Ok(stamps)
            } else {
                Err(format!(
                    "the helper stamped {} folders of {}",
                    stamps.len(),
                    pairs.len()
                ))
            }
        });
        let stamps = match answer {
            Ok(stamps) => stamps,
            Err(message) => {
                if failing.as_deref() != Some(message.as_str()) {
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "component": "hided", "kind": "device_watch.failed",
                            "device": current.device_id, "message": message,
                        })
                    );
                    failing = Some(message);
                }
                continue;
            }
        };
        failing = None;
        let now: Stamps = pairs
            .into_iter()
            .map(|(folder, _)| folder)
            .zip(stamps)
            .collect();
        let key = (current.device_id.clone(), current.root.clone());
        match recorded.as_mut() {
            Some((recorded_key, before)) if *recorded_key == key => {
                for path in changed_folders(before, &now) {
                    let _ = frames.send(frame(&current.device_id, &path));
                }
                *before = now;
            }
            _ => recorded = Some((key, now)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            target_from_value(&snapshot("explorer", "ready")),
            Some(DeviceTarget {
                device_id: "mac".to_owned(),
                root: "/r".to_owned(),
                folders: vec![
                    "/r".to_owned(),
                    "/r/src".to_owned(),
                    "/r/src/deep".to_owned()
                ],
            })
        );
        assert_eq!(target_from_value(&snapshot("changes", "ready")), None);
        assert_eq!(target_from_value(&snapshot("explorer", "connecting")), None);
        let mut local = snapshot("explorer", "ready");
        local["rest"]["navigator"]["focused_device_id"] = "local".into();
        assert_eq!(target_from_value(&local), None);
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
}
