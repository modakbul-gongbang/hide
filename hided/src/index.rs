//! The ⌘P file index (PRD B12, D-05, D-08).
//!
//! One index per checkout root, built lazily the first time the operator opens
//! the palette and kept until the root leaves the registration. The walk honors
//! `.gitignore` (and `.git/info/exclude` and the global ignore file) through the
//! `ignore` crate, never a `git` process, which the performance rules forbid on
//! a shell path. Paths are capped at `INDEX_CAP`; the cap is reported rather
//! than grown, and the palette says the list is truncated.
//!
//! Ranking mirrors the Swift scorer (`WorkspaceFileSearchIndex.fuzzyScore`):
//! the query's characters must appear in order, an earlier match scores higher,
//! adjacent and word-boundary matches score higher, and a shorter path wins a
//! tie. The result limit is the Swift sheet's 80.

use cap_std::fs::Dir;
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Files one checkout's index holds; past it the index is reported truncated.
pub const INDEX_CAP: usize = 50_000;
const DIRECTORY_CAP: usize = 50_000;

/// Matches the palette shows, the Swift sheet's limit.
pub const RESULT_LIMIT: usize = 80;

/// The fuzzy score of `query` against `candidate`, or `None` when the query's
/// characters are not found in order. `candidate` is lowercased by the caller
/// and `query` is already trimmed and lowercased, exactly as Swift prepares
/// them, so the two scorers agree.
pub fn fuzzy_score(candidate: &str, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let chars: Vec<char> = candidate.chars().collect();
    let mut cursor = 0usize;
    let mut score: i64 = 0;
    let mut previous: Option<usize> = None;
    for wanted in query.chars() {
        let found = chars[cursor..].iter().position(|c| *c == wanted)? + cursor;
        let offset = found;
        score += 100 - offset.min(90) as i64;
        if previous.is_some_and(|last| last + 1 == found) {
            score += 35;
        }
        if found == 0 || matches!(chars[found - 1], '/' | '_' | '-' | ' ' | '.') {
            score += 25;
        }
        previous = Some(found);
        cursor = found + 1;
    }
    score -= chars.len() as i64;
    Some(score)
}

/// The ranked matches for `query`, best first. An empty query lists the index
/// as it is (the walk's natural order), which is what the Swift sheet does.
pub fn rank(paths: &[String], query: &str, limit: usize) -> Vec<(String, i64)> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return paths
            .iter()
            .take(limit)
            .map(|path| (path.clone(), 0))
            .collect();
    }
    let mut scored: Vec<(String, i64)> = paths
        .iter()
        .filter_map(|path| {
            fuzzy_score(&path.to_lowercase(), &needle).map(|score| (path.clone(), score))
        })
        .collect();
    scored.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| crate::boundary::natural_cmp(&left.0, &right.0))
    });
    scored.truncate(limit);
    scored
}

/// The answer to one palette query.
pub enum IndexAnswer {
    /// The first request kicked a build; the next one has the list.
    Indexing,
    Ready {
        entries: Vec<String>,
        truncated: bool,
    },
}

struct Entry {
    data: Mutex<Option<Arc<IndexData>>>,
    building: AtomicBool,
}

struct IndexData {
    paths: Vec<String>,
    truncated: bool,
}

/// The daemon's index cache: one lazy index per registered root, bounded to the
/// roots currently registered (engineering 15). A root that is removed drops
/// its index on the next reconcile.
pub struct IndexService {
    entries: Mutex<HashMap<PathBuf, Arc<Entry>>>,
}

impl IndexService {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Keeps only the roots still registered, so a removed checkout's index
    /// does not live on.
    pub fn set_roots(&self, roots: &[PathBuf]) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|path, _| roots.contains(path));
    }

    /// The ranked matches for one root, building the index on the first call.
    pub fn query(&self, root: &Path, opened: File, query: &str) -> IndexAnswer {
        let entry = {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(entries.entry(root.to_path_buf()).or_insert_with(|| {
                Arc::new(Entry {
                    data: Mutex::new(None),
                    building: AtomicBool::new(false),
                })
            }))
        };
        let ready = entry
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(data) = ready else {
            if !entry.building.swap(true, Ordering::SeqCst) {
                let root = root.to_path_buf();
                let target = Arc::clone(&entry);
                // The guard clears the flag on every exit, including a panic
                // in the walk: otherwise the index would answer `indexing`
                // forever and the palette would poll with nothing to show.
                struct Building(Arc<Entry>);
                impl Drop for Building {
                    fn drop(&mut self) {
                        self.0.building.store(false, Ordering::SeqCst);
                    }
                }
                let guard = Building(Arc::clone(&entry));
                if std::thread::Builder::new()
                    .name("hided-index".to_owned())
                    .spawn(move || {
                        let _guard = guard;
                        let data = Arc::new(build(&root, opened));
                        *target
                            .data
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(data);
                    })
                    .is_err()
                {
                    // The guard is dropped here, so a thread that never
                    // started also clears the flag.
                }
            }
            return IndexAnswer::Indexing;
        };
        IndexAnswer::Ready {
            entries: rank(&data.paths, query, RESULT_LIMIT)
                .into_iter()
                .map(|(path, _)| path)
                .collect(),
            truncated: data.truncated,
        }
    }
}

impl Default for IndexService {
    fn default() -> Self {
        Self::new()
    }
}

/// Walks `root` honoring the ignore files, and returns the relative file paths
/// with the truncation flag. The walk is the only place a checkout is read
/// whole; it runs on its own thread, never under the runtime lock.
fn build(root: &Path, opened: File) -> IndexData {
    // Every recursive step opens from an already opened directory. A checkout
    // pathname replaced after the query cannot redirect the walk.
    let dir = Dir::from_std_file(opened);
    let (global, _) = GitignoreBuilder::new(root).build_global();
    let exclude = ignore_file(&dir, root, ".git/info/exclude");
    let mut stack = vec![(dir, PathBuf::new(), exclude.into_iter().collect::<Vec<_>>())];
    let mut paths = Vec::new();
    let mut truncated = false;
    let mut directories = 0usize;
    'walk: while let Some((dir, relative_dir, inherited)) = stack.pop() {
        directories += 1;
        if directories > DIRECTORY_CAP {
            truncated = true;
            break;
        }
        let absolute_dir = root.join(&relative_dir);
        let mut rules = inherited;
        if let Some(matcher) = ignore_file(&dir, &absolute_dir, ".gitignore") {
            rules.push(matcher);
        }
        if let Some(matcher) = ignore_file(&dir, &absolute_dir, ".ignore") {
            rules.push(matcher);
        }
        let Ok(entries) = dir.entries() else {
            truncated = true;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                truncated = true;
                continue;
            };
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            let relative = relative_dir.join(name);
            let absolute = root.join(&relative);
            let Ok(kind) = entry.file_type() else {
                truncated = true;
                continue;
            };
            if ignored(&rules, &global, &absolute, kind.is_dir()) {
                continue;
            }
            if kind.is_dir() {
                if directories + stack.len() >= DIRECTORY_CAP {
                    truncated = true;
                    continue;
                }
                match entry.open_dir() {
                    Ok(child) => stack.push((child, relative, rules.clone())),
                    Err(_) => truncated = true,
                }
            } else if kind.is_file() {
                if paths.len() == INDEX_CAP {
                    truncated = true;
                    break 'walk;
                }
                paths.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    paths.sort_by(|left, right| crate::boundary::natural_cmp(left, right));
    IndexData { paths, truncated }
}

fn ignore_file(dir: &Dir, base: &Path, name: &str) -> Option<Gitignore> {
    let mut file = dir.open(name).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let mut builder = GitignoreBuilder::new(base);
    let contents = String::from_utf8_lossy(&bytes);
    for line in contents.lines() {
        let _ = builder.add_line(None, line);
    }
    builder.build().ok()
}

fn ignored(rules: &[Gitignore], global: &Gitignore, path: &Path, is_dir: bool) -> bool {
    for matcher in rules.iter().rev().chain(std::iter::once(global)) {
        match matcher.matched(path, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn open_test_dir(root: &Path) -> File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x00000001 | 0x00000002)
            .custom_flags(0x02000000)
            .open(root)
            .unwrap()
    }

    #[cfg(not(windows))]
    fn open_test_dir(root: &Path) -> File {
        File::open(root).unwrap()
    }

    #[test]
    fn the_scorer_mirrors_the_swift_one() {
        // The characters have to appear in order.
        assert_eq!(fuzzy_score("src/main.rs", "nope"), None);
        assert!(fuzzy_score("src/main.rs", "smr").is_some());
        // An adjacent and word-boundary match beats a scattered one.
        let tight = fuzzy_score("src/main.rs", "main").unwrap();
        let loose = fuzzy_score("server/migrations/init.rs", "main").unwrap();
        assert!(tight > loose, "{tight} > {loose}");
        // A prefix match scores higher than a late one.
        assert!(
            fuzzy_score("readme.md", "r").unwrap() > fuzzy_score("docs/readme.md", "r").unwrap()
        );
    }

    #[test]
    fn ranking_orders_by_score_then_name_and_limits_the_list() {
        let paths = vec![
            "src/main.rs".to_owned(),
            "src/mnemonic.rs".to_owned(),
            "docs/readme.md".to_owned(),
        ];
        let ranked = rank(&paths, "main", 80);
        assert_eq!(ranked[0].0, "src/main.rs");
        assert!(!ranked.iter().any(|(path, _)| path == "docs/readme.md"));
        let limited = rank(&paths, "", 2);
        assert_eq!(
            limited.len(),
            2,
            "an empty query lists the index up to the limit"
        );
    }

    #[test]
    fn the_walk_honors_gitignore_and_hides_git() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("ignored.log"), "x").unwrap();
        std::fs::write(root.join(".gitignore"), "*.log\n").unwrap();
        let data = build(root, open_test_dir(root));
        assert!(data.paths.contains(&"src/main.rs".to_owned()));
        assert!(
            data.paths.contains(&".gitignore".to_owned()),
            "hidden names are indexed"
        );
        assert!(!data.paths.iter().any(|path| path.ends_with(".log")));
        assert!(
            !data
                .paths
                .iter()
                .any(|path| path == ".git" || path.starts_with(".git/"))
        );
        assert!(!data.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn an_opened_root_is_not_redirected_by_a_path_replacement() {
        use std::os::unix::fs::symlink;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path().join("checkout");
        let outside = sandbox.path().join("outside");
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(root.join("inside.txt"), "inside").unwrap();
        std::fs::write(outside.join("outside.txt"), "outside").unwrap();
        let opened = std::fs::File::open(&root).unwrap();
        std::fs::rename(&root, sandbox.path().join("moved")).unwrap();
        symlink(&outside, &root).unwrap();
        let data = build(&root, opened);
        assert_eq!(data.paths, vec!["inside.txt"]);
    }
}
