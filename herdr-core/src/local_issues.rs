//! Local issues: the issue source for a project that keeps its issues on this
//! Mac rather than on GitHub (the Tasks board's `Local` adapter, `tasks.rs`).
//!
//! Every project's local issues live in one file in hided's state directory,
//! `local-issues.json` beside `core-state.json`, keyed by the project's path. The runtime reads it once when it is created, before any
//! lock exists, holds the store in memory, applies a change there at once,
//! and hands the whole store to a save thread, so no file I/O happens under
//! the runtime mutex (the UI state's own save pattern, `write_ui_state`).
//!
//! An issue is numbered per project from 1 and shown as `L-<n>`; a number is
//! never reused, so a link written into a branch keeps naming the same issue.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The file format version this code writes and reads.
const VERSION: u32 = 1;
/// Issues one project may hold. Creating past it is refused, not truncated.
pub const ISSUES_PER_PROJECT: usize = 2_000;
/// Projects the store may hold.
pub const PROJECTS: usize = 500;
/// A title is one line of at most this many characters.
pub const TITLE_LIMIT: usize = 256;
/// A body is at most this many bytes.
pub const BODY_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalIssue {
    pub number: u32,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub open: bool,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalIssueProject {
    /// The number the next issue takes; never lowered, so no number repeats.
    pub next_number: u32,
    pub issues: Vec<LocalIssue>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalIssueStore {
    #[serde(default)]
    pub version: u32,
    /// By project path.
    #[serde(default)]
    pub projects: BTreeMap<String, LocalIssueProject>,
}

/// `L-3`, the id a local issue shows.
pub fn display_id(number: u32) -> String {
    format!("L-{number}")
}

/// The number in an id an operator or a branch wrote for a local issue:
/// `L-3`, `l-3`, `#3` or `3`.
pub fn parse_id(value: &str) -> Option<u32> {
    let value = value.trim();
    let digits = value
        .strip_prefix("L-")
        .or_else(|| value.strip_prefix("l-"))
        .or_else(|| value.strip_prefix('#'))
        .unwrap_or(value);
    digits.parse::<u32>().ok().filter(|number| *number > 0)
}

/// A title as the store keeps it: one line, trimmed, within the limit.
pub fn validated_title(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("이슈 제목을 입력하세요.".into());
    }
    if title.contains(['\n', '\r']) {
        return Err("이슈 제목은 한 줄이어야 합니다.".into());
    }
    if title.chars().count() > TITLE_LIMIT {
        return Err(format!("이슈 제목은 {TITLE_LIMIT}자 이하여야 합니다."));
    }
    Ok(title.to_owned())
}

pub fn validated_body(body: &str) -> Result<String, String> {
    if body.len() > BODY_LIMIT {
        return Err(format!(
            "이슈 내용은 {} KB 이하여야 합니다.",
            BODY_LIMIT / 1024
        ));
    }
    Ok(body.trim_end().to_owned())
}

impl LocalIssueStore {
    pub fn project(&self, path: &str) -> Option<&LocalIssueProject> {
        self.projects.get(path)
    }

    pub fn issue(&self, path: &str, number: u32) -> Option<&LocalIssue> {
        self.project(path)?
            .issues
            .iter()
            .find(|issue| issue.number == number)
    }

    /// Adds an open issue to a project and returns its number.
    pub fn create(
        &mut self,
        path: &str,
        title: &str,
        body: &str,
        now_unix_ms: u64,
    ) -> Result<u32, String> {
        let title = validated_title(title)?;
        let body = validated_body(body)?;
        if !self.projects.contains_key(path) && self.projects.len() >= PROJECTS {
            return Err(format!(
                "로컬 이슈는 {PROJECTS}개 프로젝트까지만 둘 수 있습니다."
            ));
        }
        let project = self.projects.entry(path.to_owned()).or_default();
        if project.issues.len() >= ISSUES_PER_PROJECT {
            return Err(format!(
                "한 프로젝트의 로컬 이슈는 {ISSUES_PER_PROJECT}개까지입니다."
            ));
        }
        let number = project.next_number.max(1);
        project.next_number = number + 1;
        project.issues.push(LocalIssue {
            number,
            title,
            body,
            open: true,
            created_at_unix_ms: now_unix_ms,
            updated_at_unix_ms: now_unix_ms,
        });
        self.version = VERSION;
        Ok(number)
    }

    /// Opens or closes an issue; `false` when there is no such issue or it
    /// already is in that state.
    pub fn set_open(&mut self, path: &str, number: u32, open: bool, now_unix_ms: u64) -> bool {
        let Some(issue) = self.projects.get_mut(path).and_then(|project| {
            project
                .issues
                .iter_mut()
                .find(|issue| issue.number == number)
        }) else {
            return false;
        };
        if issue.open == open {
            return false;
        }
        issue.open = open;
        issue.updated_at_unix_ms = now_unix_ms;
        true
    }
}

/// Reads the store. A file that is not there is an empty store; one that
/// cannot be read or parsed is an error, so a damaged file is never
/// overwritten by an empty one.
pub fn load(path: &Path) -> Result<LocalIssueStore, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LocalIssueStore {
                version: VERSION,
                ..LocalIssueStore::default()
            });
        }
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let store: LocalIssueStore =
        serde_json::from_slice(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if store.version > VERSION {
        return Err(format!(
            "{} was written by a newer Hide (version {})",
            path.display(),
            store.version
        ));
    }
    Ok(store)
}

/// Writes the whole store through a temporary file and a rename, so a crash
/// mid-write leaves the previous file intact.
pub fn save(path: &Path, store: &LocalIssueStore) -> Result<(), String> {
    let directory = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let bytes = serde_json::to_vec_pretty(store).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)
        .map_err(|error| format!("{}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        format!("{}: {error}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_never_repeat_and_ids_read_back() {
        let mut store = LocalIssueStore::default();
        assert_eq!(store.create("/p", "첫 이슈", "", 1).unwrap(), 1);
        assert_eq!(store.create("/p", "둘째", "본문", 2).unwrap(), 2);
        assert_eq!(store.create("/q", "다른 프로젝트", "", 3).unwrap(), 1);
        store.projects.get_mut("/p").unwrap().issues.remove(1);
        assert_eq!(store.create("/p", "셋째", "", 4).unwrap(), 3);
        assert_eq!(display_id(3), "L-3");
        for written in ["L-3", "l-3", "#3", "3", " L-3 "] {
            assert_eq!(parse_id(written), Some(3), "{written}");
        }
        for invalid in ["L-0", "L-", "x", "L-3a", ""] {
            assert_eq!(parse_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn titles_and_bodies_are_bounded() {
        let mut store = LocalIssueStore::default();
        assert!(store.create("/p", "  ", "", 1).is_err());
        assert!(store.create("/p", "두\n줄", "", 1).is_err());
        assert!(
            store
                .create("/p", &"가".repeat(TITLE_LIMIT + 1), "", 1)
                .is_err()
        );
        assert!(
            store
                .create("/p", "ok", &"a".repeat(BODY_LIMIT + 1), 1)
                .is_err()
        );
        assert!(store.projects.is_empty());
    }

    #[test]
    fn a_project_past_its_cap_is_refused() {
        let mut store = LocalIssueStore::default();
        store.projects.insert(
            "/p".into(),
            LocalIssueProject {
                next_number: ISSUES_PER_PROJECT as u32 + 1,
                issues: (1..=ISSUES_PER_PROJECT as u32)
                    .map(|number| LocalIssue {
                        number,
                        title: "t".into(),
                        open: true,
                        ..LocalIssue::default()
                    })
                    .collect(),
            },
        );
        assert!(store.create("/p", "one more", "", 1).is_err());
    }

    #[test]
    fn the_store_survives_a_round_trip_and_a_missing_file_is_empty() {
        let directory = std::env::temp_dir().join(format!(
            "hide-local-issues-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = directory.join("local-issues.json");
        assert!(load(&path).unwrap().projects.is_empty());
        let mut store = LocalIssueStore::default();
        store.create("/p", "한국어 제목", "본문", 7).unwrap();
        assert!(store.set_open("/p", 1, false, 8));
        assert!(!store.set_open("/p", 1, false, 9));
        save(&path, &store).unwrap();
        assert_eq!(load(&path).unwrap(), store);
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load(&path).is_err());
        let _ = std::fs::remove_dir_all(&directory);
    }
}
