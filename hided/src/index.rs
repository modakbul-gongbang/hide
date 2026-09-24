//! The ⌘P file index (PRD S3 B12, D-05, D-08; S5.5 B5-B6).
//!
//! One index per checkout root on each device, built lazily the first time the
//! operator opens the palette and kept until the root leaves the catalog. The
//! walk is `hide_host::index::walk`: this machine's runs in process on a
//! worker, a device's is its helper's `Call::Index`. The list is capped at
//! `hide_host::index::INDEX_CAP` and the palette says when it is truncated.
//!
//! Ranking mirrors the Swift scorer (`WorkspaceFileSearchIndex.fuzzyScore`):
//! the query's characters must appear in order, an earlier match scores higher,
//! adjacent and word-boundary matches score higher, and a shorter path wins a
//! tie. The result limit is the Swift sheet's 80.

use hide_host::index::Walked;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
            .then_with(|| hide_host::list::natural_cmp(&left.0, &right.0))
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
    /// The walk could not be made, with the reason; the next query tries again.
    Failed(String),
}

/// One index: a device and a root path on it.
type IndexKey = (String, String);

struct Entry {
    data: Mutex<Option<Result<Arc<Walked>, String>>>,
    building: AtomicBool,
}

/// The daemon's index cache: one lazy index per checkout root of each device,
/// bounded to the roots the catalog currently carries (engineering 15). A root
/// that is removed drops its index on the next reconcile.
pub struct IndexService {
    entries: Mutex<HashMap<IndexKey, Arc<Entry>>>,
}

impl IndexService {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Keeps only the roots still in the catalog, so a removed checkout's
    /// index does not live on.
    pub fn set_roots(&self, roots: &[IndexKey]) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|key, _| roots.contains(key));
    }

    /// The ranked matches for one root of one device. The first call starts
    /// `walk` on its own thread and answers `Indexing`; a failed walk is
    /// answered once and then forgotten, so the next query walks again.
    pub fn query(
        &self,
        device: &str,
        root: &str,
        query: &str,
        walk: impl FnOnce() -> Result<Walked, String> + Send + 'static,
    ) -> IndexAnswer {
        let entry = {
            let mut entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(
                entries
                    .entry((device.to_owned(), root.to_owned()))
                    .or_insert_with(|| {
                        Arc::new(Entry {
                            data: Mutex::new(None),
                            building: AtomicBool::new(false),
                        })
                    }),
            )
        };
        let ready = {
            let mut data = entry
                .data
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match data.as_ref() {
                Some(Err(_)) => data.take(),
                other => other.cloned(),
            }
        };
        match ready {
            Some(Ok(data)) => IndexAnswer::Ready {
                entries: rank(&data.paths, query, RESULT_LIMIT)
                    .into_iter()
                    .map(|(path, _)| path)
                    .collect(),
                truncated: data.truncated,
            },
            Some(Err(message)) => IndexAnswer::Failed(message),
            None => {
                if !entry.building.swap(true, Ordering::SeqCst) {
                    let target = Arc::clone(&entry);
                    // The guard clears the flag on every exit, including a
                    // panic in the walk: otherwise the index would answer
                    // `indexing` forever and the palette would poll with
                    // nothing to show.
                    struct Building(Arc<Entry>);
                    impl Drop for Building {
                        fn drop(&mut self) {
                            self.0.building.store(false, Ordering::SeqCst);
                        }
                    }
                    let guard = Building(Arc::clone(&entry));
                    // A thread that never started drops the guard here, which
                    // clears the flag too.
                    let _ = std::thread::Builder::new()
                        .name("hided-index".to_owned())
                        .spawn(move || {
                            let _guard = guard;
                            let data = walk().map(Arc::new);
                            *target
                                .data
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(data);
                        });
                }
                IndexAnswer::Indexing
            }
        }
    }
}

impl Default for IndexService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer_after_walk(service: &IndexService, walk: Result<Walked, String>) -> IndexAnswer {
        assert!(matches!(
            service.query("device", "/root", "", move || walk),
            IndexAnswer::Indexing
        ));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let answer = service.query("device", "/root", "", || Ok(Walked::default()));
            if !matches!(answer, IndexAnswer::Indexing) {
                return answer;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the walk never landed"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_failed_walk_is_reported_once_and_walked_again() {
        let service = IndexService::new();
        let failed = answer_after_walk(&service, Err("helper gone".to_owned()));
        assert!(matches!(failed, IndexAnswer::Failed(message) if message == "helper gone"));
        let again = answer_after_walk(
            &service,
            Ok(Walked {
                paths: vec!["a.txt".to_owned()],
                truncated: false,
            }),
        );
        assert!(matches!(again, IndexAnswer::Ready { entries, .. } if entries == ["a.txt"]));
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
}
