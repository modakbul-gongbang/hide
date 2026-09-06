//! Worktree disk usage, measured sequentially on the existing background worker.
//! Requests come only from opening or explicitly refreshing the Git section.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::model::DiskUsageSnapshot;
use crate::reader::BackgroundRead;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiskRequest {
    /// Worktrees to measure, empty while the Git section is hidden.
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

fn read(request: &DiskRequest) -> Vec<DiskUsageSnapshot> {
    request
        .paths
        .iter()
        .map(|path| {
            let mut measured = measure(path);
            measured.measured_at_unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|time| time.as_millis() as u64);
            measured
        })
        .collect()
}

fn measure(path: &Path) -> DiskUsageSnapshot {
    let path_text = path.to_string_lossy().into_owned();
    if !path.is_dir() {
        return DiskUsageSnapshot {
            path: Some(path_text),
            unavailable_reason: Some("This checkout is not on disk.".to_owned()),
            ..DiskUsageSnapshot::default()
        };
    }

    // One `du` walks the tree once and reports the total and every immediate
    // child, so the largest folder costs nothing beyond the total that is
    // wanted anyway. `-d 1` keeps the output to one line per child instead of
    // one per file; it may not be combined with `-s`, which BSD `du` rejects
    // with a usage message rather than an error anyone would recognise.
    let output = match Command::new("du")
        .args(["-k", "-d", "1"])
        .arg(path)
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return DiskUsageSnapshot {
                path: Some(path_text),
                unavailable_reason: Some(format!("du could not be run: {error}")),
                ..DiskUsageSnapshot::default()
            };
        }
    };
    // A failed walk is not a complete size, even when du emitted a partial total.
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let measured = parse_du(&text, path);
    if !output.status.success() || measured.total_bytes.is_none() {
        crate::diagnostic!(serde_json::json!({
                "component": "disk",
                "kind": "measure.failed",
                "path": path_text,
                "message": String::from_utf8_lossy(&output.stderr).trim(),
            })
        );
        return DiskUsageSnapshot {
            path: Some(path_text),
            unavailable_reason: Some(format!(
                "du measurement failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            ..DiskUsageSnapshot::default()
        };
    }
    DiskUsageSnapshot {
        path: Some(path_text),
        ..measured
    }
}

/// Reads `du -sk -d 1` output: one `<kilobytes>\t<path>` line per immediate
/// child and one for the directory itself, in any order.
///
/// The root's own line is the total; the largest other line is the folder the
/// card names beside it. Sizes are kilobytes, converted here so nothing
/// downstream has to remember the unit.
pub fn parse_du(output: &str, root: &Path) -> DiskUsageSnapshot {
    let root_text = root.to_string_lossy();
    let root_text = root_text.trim_end_matches('/');
    let mut total_bytes = None;
    let mut largest: Option<(String, u64)> = None;

    for line in output.lines() {
        let Some((size, path)) = line.split_once('\t') else {
            continue;
        };
        let Ok(kilobytes) = size.trim().parse::<u64>() else {
            continue;
        };
        let path = path.trim_end_matches('/');
        let bytes = kilobytes * 1024;
        if path == root_text {
            total_bytes = Some(bytes);
            continue;
        }
        let Some(name) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if largest.as_ref().is_none_or(|(_, known)| bytes > *known) {
            largest = Some((name.to_owned(), bytes));
        }
    }

    DiskUsageSnapshot {
        path: None,
        total_bytes,
        largest_child_name: largest.as_ref().map(|(name, _)| name.clone()),
        largest_child_bytes: largest.map(|(_, bytes)| bytes),
        unavailable_reason: None,
        measured_at_unix_ms: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_line_is_the_total_and_the_biggest_child_is_named() {
        let output =
            "12\t/checkout/src\n2048\t/checkout/target\n64\t/checkout/docs\n2200\t/checkout\n";
        let measured = parse_du(output, Path::new("/checkout"));
        assert_eq!(measured.total_bytes, Some(2200 * 1024));
        assert_eq!(measured.largest_child_name.as_deref(), Some("target"));
        assert_eq!(measured.largest_child_bytes, Some(2048 * 1024));
    }

    /// A trailing slash on the requested path must still match `du`'s own
    /// spelling of it, or the total would be mistaken for a child folder.
    #[test]
    fn a_trailing_slash_still_identifies_the_root_line() {
        let output = "10\t/checkout/src\n30\t/checkout\n";
        let measured = parse_du(output, Path::new("/checkout/"));
        assert_eq!(measured.total_bytes, Some(30 * 1024));
        assert_eq!(measured.largest_child_name.as_deref(), Some("src"));
    }

    #[test]
    fn a_checkout_with_no_subfolders_reports_a_total_and_no_child() {
        let measured = parse_du("8\t/checkout\n", Path::new("/checkout"));
        assert_eq!(measured.total_bytes, Some(8 * 1024));
        assert_eq!(measured.largest_child_name, None);
    }

    /// Runs the real `du` against a real directory.
    ///
    /// This exists because the flags cannot be checked any other way: `-sk -d
    /// 1` compiled, ran, and returned a BSD usage message on every
    /// measurement, and every parse test still passed because they parse
    /// output this reader never produced. The class of bug is "a subprocess
    /// invocation the compiler cannot check", and the only thing that catches
    /// it is invoking it (PRINCIPLES 13).
    #[test]
    fn the_real_du_invocation_measures_a_real_directory() {
        let root = std::env::temp_dir().join(format!(
            "hide-disk-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let big = root.join("big");
        let small = root.join("small");
        std::fs::create_dir_all(&big).expect("big directory");
        std::fs::create_dir_all(&small).expect("small directory");
        std::fs::write(big.join("payload"), vec![0_u8; 256 * 1024]).expect("big file");
        std::fs::write(small.join("payload"), vec![0_u8; 4 * 1024]).expect("small file");

        let measured = read(&DiskRequest {
            paths: vec![root.clone(), small.clone()],
            generation: 0,
        });
        assert_eq!(measured.len(), 2);
        assert_eq!(
            measured[1].path.as_deref(),
            Some(small.to_string_lossy().as_ref())
        );
        assert!(measured[1].total_bytes.unwrap() >= 4 * 1024);
        assert!(measured[1].measured_at_unix_ms >= measured[0].measured_at_unix_ms);
        let measured = &measured[0];
        assert!(measured.measured_at_unix_ms.is_some());
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(
            measured.unavailable_reason, None,
            "du ran and was understood"
        );
        assert_eq!(
            measured.path.as_deref(),
            Some(root.to_string_lossy().as_ref())
        );
        let total = measured.total_bytes.expect("a total was reported");
        assert!(total >= 256 * 1024, "the total covers the tree: {total}");
        assert_eq!(measured.largest_child_name.as_deref(), Some("big"));
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
