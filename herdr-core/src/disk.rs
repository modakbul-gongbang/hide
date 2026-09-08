//! Worktree disk usage, measured sequentially on the existing background worker.
//! Git and Overview share this lane; opening or explicit refresh requests a measurement.

use std::path::PathBuf;
use std::time::Duration;

use crate::model::DiskUsageSnapshot;
use crate::reader::BackgroundRead;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiskRequest {
    /// Checkout roots and the shared Git directory, partitioned without overlap.
    pub paths: Vec<PathBuf>,
    /// Bumped on section opening and explicit refresh.
    pub generation: u64,
}

pub struct DiskReader {
    inner: BackgroundRead<DiskRequest, Vec<DiskUsageSnapshot>>,
}

impl DiskReader {
    pub fn new() -> Self {
        Self {
            inner: BackgroundRead::on_change(Duration::ZERO, read),
        }
    }

    pub fn read_if_due(&mut self, request: DiskRequest) -> Option<Vec<DiskUsageSnapshot>> {
        self.inner.poll(request)
    }
}

impl Default for DiskReader {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn read(request: &DiskRequest) -> Vec<DiskUsageSnapshot> {
    use std::collections::{BTreeMap, HashSet};
    use std::os::unix::fs::MetadataExt;
    // The deepest explicitly requested root owns its subtree. Shared Git and
    // nested linked worktrees are therefore excluded from the main component.
    // One inode set also counts hard links once across sibling components.
    let mut roots = request.paths.clone();
    roots.sort_by(|a, b| {
        b.components()
            .count()
            .cmp(&a.components().count())
            .then(a.cmp(b))
    });
    roots.dedup();
    let root_set: HashSet<_> = roots.iter().cloned().collect();
    let mut seen = HashSet::new();
    let measured_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|t| t.as_millis() as u64);
    let mut result = Vec::new();
    let started = std::time::Instant::now();
    let mut visited = 0usize;
    const ENTRY_LIMIT: usize = 1_000_000;
    for root in &roots {
        let mut bytes = 0u64;
        let mut children = BTreeMap::<String, u64>::new();
        let mut stack = vec![root.clone()];
        let mut failure = None;
        // Reject alias roots rather than following a symlink outside the
        // declared measurement boundary. Descendant links count their own
        // allocated blocks and are never traversed.
        if std::fs::canonicalize(root).is_ok_and(|canonical| canonical != *root) {
            failure = Some(
                "The measurement root is an alias. Refresh with the canonical checkout path."
                    .into(),
            );
            stack.clear();
        }
        while let Some(path) = stack.pop() {
            visited += 1;
            if visited > ENTRY_LIMIT || started.elapsed() > Duration::from_secs(30) {
                failure = Some("Measurement exceeded its 30 second / one million entry limit. This component is unavailable; measure again after reducing the folder size.".into());
                break;
            }
            if path != *root && root_set.contains(&path) {
                continue;
            }
            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(value) => value,
                Err(error) => {
                    failure = Some(format!("Disk measurement incomplete: {error}"));
                    break;
                }
            };
            if !seen.insert((metadata.dev(), metadata.ino())) {
                continue;
            }
            let allocated = metadata.blocks().saturating_mul(512);
            bytes = bytes.saturating_add(allocated);
            if let Ok(relative) = path.strip_prefix(root)
                && let Some(name) = relative.components().next()
            {
                *children
                    .entry(name.as_os_str().to_string_lossy().into_owned())
                    .or_default() += allocated;
            }
            if metadata.is_dir() {
                match std::fs::read_dir(&path) {
                    Ok(entries) => {
                        let mut descendants = Vec::new();
                        for entry in entries {
                            if descendants.len() + stack.len() + visited >= ENTRY_LIMIT {
                                failure = Some(
                                    "Measurement exceeded its one million entry limit.".into(),
                                );
                                break;
                            }
                            match entry {
                                Ok(entry) => descendants.push(entry.path()),
                                Err(error) => {
                                    failure = Some(format!("Disk measurement incomplete: {error}"));
                                }
                            }
                        }
                        descendants.sort();
                        stack.extend(descendants.into_iter().rev());
                    }
                    Err(error) => {
                        failure = Some(format!("Disk measurement incomplete: {error}"));
                        break;
                    }
                }
            }
        }
        let largest = children.into_iter().max_by_key(|(_, size)| *size);
        result.push(DiskUsageSnapshot {
            path: Some(root.to_string_lossy().into_owned()),
            total_bytes: failure.is_none().then_some(bytes),
            largest_child_name: largest.as_ref().map(|(name, _)| name.clone()),
            largest_child_bytes: largest.map(|(_, size)| size),
            unavailable_reason: failure,
            measured_at_unix_ms: measured_at,
        });
    }
    // Preserve caller order for stable identities and unchanged snapshot reuse.
    result.sort_by_key(|row| {
        request
            .paths
            .iter()
            .position(|p| Some(p.to_string_lossy().as_ref()) == row.path.as_deref())
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_disk_partitions_nested_worktrees_and_shared_git_once() {
        let root = fixture("partition");
        std::fs::create_dir_all(root.join("linked")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("main-data"), vec![1; 8192]).unwrap();
        std::fs::write(root.join("linked/data"), vec![2; 16384]).unwrap();
        std::fs::write(root.join(".git/data"), vec![3; 32768]).unwrap();
        use std::os::unix::fs::MetadataExt;
        let expected: u64 = [
            root.clone(),
            root.join("linked"),
            root.join(".git"),
            root.join("main-data"),
            root.join("linked/data"),
            root.join(".git/data"),
        ]
        .iter()
        .map(|p| std::fs::metadata(p).unwrap().blocks() * 512)
        .sum();
        let measurements = read(&DiskRequest {
            paths: vec![root.clone(), root.join("linked"), root.join(".git")],
            generation: 0,
        });
        let total: u64 = measurements.iter().map(|d| d.total_bytes.unwrap()).sum();
        std::fs::remove_dir_all(root).unwrap();
        assert_eq!(
            total, expected,
            "Every allocated block belongs to one project component"
        );
    }

    #[test]
    fn hardlinks_count_once_and_symlinks_do_not_escape_or_cycle() {
        use std::os::unix::fs::{MetadataExt, symlink};
        let root = fixture("links");
        let outside = fixture("outside");
        std::fs::create_dir_all(root.join("linked")).unwrap();
        std::fs::write(root.join("linked/data"), vec![2; 16384]).unwrap();
        std::fs::hard_link(root.join("linked/data"), root.join("alias")).unwrap();
        std::fs::write(outside.join("secret"), vec![3; 65536]).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        symlink(&root, root.join("cycle")).unwrap();
        let expected: u64 = [
            root.clone(),
            root.join("linked"),
            root.join("linked/data"),
            root.join("escape"),
            root.join("cycle"),
        ]
        .iter()
        .map(|p| std::fs::symlink_metadata(p).unwrap().blocks() * 512)
        .sum();
        let values = read(&DiskRequest {
            paths: vec![root.clone(), root.join("linked")],
            generation: 0,
        });
        assert_eq!(
            values.iter().map(|v| v.total_bytes.unwrap()).sum::<u64>(),
            expected
        );
        assert_eq!(
            values[1].total_bytes,
            Some(
                std::fs::metadata(root.join("linked")).unwrap().blocks() * 512
                    + std::fs::metadata(root.join("linked/data"))
                        .unwrap()
                        .blocks()
                        * 512
            )
        );
        let alias = read(&DiskRequest {
            paths: vec![root.join("escape")],
            generation: 0,
        });
        assert_eq!(alias[0].total_bytes, None);
        assert!(alias[0].unavailable_reason.is_some());
        assert!(outside.join("secret").exists());
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn partial_measurement_retains_target_failure_without_a_complete_total() {
        let root = fixture("partial");
        std::fs::write(root.join("data"), vec![1; 8192]).unwrap();
        let rows = read(&DiskRequest {
            paths: vec![root.clone(), root.join("missing")],
            generation: 0,
        });
        assert!(rows[0].total_bytes.is_some());
        assert!(rows[1].total_bytes.is_none());
        assert!(rows[1].unavailable_reason.is_some());
        assert_eq!(
            rows.iter().map(|row| row.total_bytes).sum::<Option<u64>>(),
            None
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "hide-disk-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::canonicalize(root).unwrap()
    }

    #[test]
    fn an_unreadable_request_measures_nothing() {
        let measured = read(&DiskRequest {
            paths: vec![PathBuf::from("/definitely/not/here/hide-test")],
            generation: 0,
        });
        let measured = &measured[0];
        assert_eq!(measured.total_bytes, None);
        assert!(measured.unavailable_reason.is_some());
    }

    #[test]
    fn no_selection_measures_nothing_at_all() {
        assert!(read(&DiskRequest::default()).is_empty());
    }
}
