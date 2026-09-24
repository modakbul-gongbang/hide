//! The file index of one opened checkout root: every file's path relative to
//! the root, for the ⌘P palette (PRD S3 B12, S5.5 B5-B6).
//!
//! The walk honors `.gitignore` (and `.git/info/exclude`, `.ignore` and the
//! global ignore file) through the `ignore` crate, never a `git` process, which
//! the performance rules forbid on a shell path. Files are capped at
//! `INDEX_CAP` and folders at `DIRECTORY_CAP`; the cap is reported rather than
//! grown. This machine's daemon runs it in process and a device's helper
//! answers `Call::Index` with the same walk, so both rank the same list.

use cap_std::fs::{Dir, OpenOptions as CapOpenOptions};
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Files one checkout's index holds; past it the index is reported truncated.
pub const INDEX_CAP: usize = 50_000;
/// Folders one walk visits; past it the index is reported truncated.
pub const DIRECTORY_CAP: usize = 50_000;

/// One walk's answer.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Walked {
    pub paths: Vec<String>,
    pub truncated: bool,
}

/// Walks the opened root honoring the ignore files, and returns the relative
/// file paths with the truncation flag. `root` is the root's real path, which
/// the ignore rules are anchored at; nothing is read through it. The walk is
/// the only place a checkout is read whole, so its caller runs it on a worker,
/// never under the runtime lock.
pub fn walk(root_dir: &Dir, root: &Path) -> Walked {
    // Keep only the verified root handle open across siblings. A wide tree
    // must not consume one descriptor for every pending directory.
    let (global, _) = GitignoreBuilder::new(root).build_global();
    let exclude = ignore_file(root_dir, root, ".git/info/exclude");
    let mut stack = vec![(PathBuf::new(), exclude.into_iter().collect::<Vec<_>>())];
    let mut paths = Vec::new();
    let mut truncated = false;
    let mut directories = 0usize;
    'walk: while let Some((relative_dir, inherited)) = stack.pop() {
        directories += 1;
        if directories > DIRECTORY_CAP {
            truncated = true;
            break;
        }
        let dir = match root_dir.open_dir(if relative_dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            &relative_dir
        }) {
            Ok(dir) => dir,
            Err(_) => {
                truncated = true;
                continue;
            }
        };
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
                stack.push((relative, rules.clone()));
            } else if kind.is_file() {
                if paths.len() == INDEX_CAP {
                    truncated = true;
                    break 'walk;
                }
                paths.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    paths.sort_by(|left, right| crate::list::natural_cmp(left, right));
    Walked { paths, truncated }
}

fn ignore_file(dir: &Dir, base: &Path, name: &str) -> Option<Gitignore> {
    let mut options = CapOpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
    }
    let mut file = dir.open_with(name, &options).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
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

    fn build(root: &Path, opened: std::fs::File) -> Walked {
        walk(&Dir::from_std_file(opened), root)
    }

    fn open_test_dir(root: &Path) -> std::fs::File {
        std::fs::File::open(root).unwrap()
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

    #[test]
    fn a_wide_checkout_keeps_all_files_without_opening_sibling_handles() {
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path();
        for index in 0..512 {
            let folder = root.join(format!("folder-{index:03}"));
            std::fs::create_dir(&folder).unwrap();
            std::fs::write(folder.join("entry.txt"), "entry").unwrap();
        }
        let data = build(root, open_test_dir(root));
        assert_eq!(data.paths.len(), 512);
        assert!(data.paths.contains(&"folder-511/entry.txt".to_owned()));
        assert!(!data.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_named_gitignore_cannot_block_the_index() {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let sandbox = tempfile::tempdir().unwrap();
        let root = sandbox.path();
        let fifo = CString::new(root.join(".gitignore").as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        std::fs::write(root.join("visible.txt"), "visible").unwrap();
        let data = build(root, open_test_dir(root));
        assert_eq!(data.paths, vec!["visible.txt"]);
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
