//! Repository discovery from the repository's own files.
//!
//! The catalog needs three facts about a directory: which working tree holds
//! it, which working tree owns the repository, and what branch is checked out.
//! `git rev-parse` answers each by reading a handful of files, and so does this
//! module, without the process. A process is what made the answer expensive:
//! the session-sync coordinator rebuilt the catalog with one `git` per fact per
//! pane directory, and on a machine where a spawn costs tens of milliseconds
//! that held Herdr's events behind seconds of `git rev-parse`. Reading the
//! files costs microseconds, so the rebuild can stay on the coordinator.
//!
//! The walk mirrors git's: from the directory upward until an entry named
//! `.git` is found, which is either the git directory itself or a file naming
//! it. A linked worktree's git directory carries a `commondir` file pointing at
//! the repository the worktree belongs to, and `HEAD` names the branch.

use std::fs;
use std::path::{Path, PathBuf};

/// One directory's repository, as `git rev-parse` would describe it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    /// The working tree holding the directory (`--show-toplevel`).
    pub root: PathBuf,
    /// The git directory of that working tree (`--git-dir`, absolute).
    pub git_dir: PathBuf,
    /// The repository's shared git directory (`--git-common-dir`, absolute).
    /// Equal to `git_dir` in the main working tree.
    pub common_dir: PathBuf,
}

impl Repository {
    /// The working tree that owns the repository: the main worktree for a
    /// linked worktree, the checkout itself otherwise.
    pub fn main_root(&self) -> PathBuf {
        self.common_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.root.clone())
    }

    /// The checked-out branch, or `None` when `HEAD` is detached.
    ///
    /// An unborn branch (a fresh `git init`) is reported by name, as
    /// `git branch --show-current` does.
    pub fn branch(&self) -> Option<String> {
        let head = fs::read_to_string(self.git_dir.join("HEAD")).ok()?;
        let reference = head.trim().strip_prefix("ref:")?.trim();
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        (!name.is_empty()).then(|| name.to_owned())
    }

    /// Reads the first logical line of `branch.<name>.description` from the shared
    /// repository config without spawning git.
    ///
    /// This intentionally parses only the section and key Hide owns. Git's
    /// quoted subsection escapes are decoded so branch names containing a
    /// quote or backslash select the same section `git config` writes.
    pub fn branch_description(&self, branch: &str) -> Result<Option<String>, String> {
        let path = self.common_dir.join("config");
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("repository config could not be read: {error}"))?;
        Ok(parse_branch_description(&text, branch))
    }

    pub fn branch_issue(&self, branch: &str) -> Result<Option<String>, String> {
        let text = fs::read_to_string(self.common_dir.join("config"))
            .map_err(|error| format!("repository config could not be read: {error}"))?;
        Ok(parse_branch_value(&text, branch, "issue"))
    }
}

fn parse_branch_description(config: &str, wanted_branch: &str) -> Option<String> {
    parse_branch_value(config, wanted_branch, "description")
}

fn parse_branch_value(config: &str, wanted_branch: &str, wanted_key: &str) -> Option<String> {
    let mut selected = false;
    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') {
            selected = parse_branch_section(line).as_deref() == Some(wanted_branch);
            continue;
        }
        if !selected || line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            // Git permits bare Boolean keys. They do not make a later
            // description in the same section unreadable.
            continue;
        };
        if !key.trim().eq_ignore_ascii_case(wanted_key) {
            continue;
        }
        let value = parse_config_value(value.trim());
        let first_line = value.lines().next().unwrap_or_default().trim_end();
        return (!first_line.is_empty()).then(|| first_line.to_owned());
    }
    None
}

fn parse_branch_section(line: &str) -> Option<String> {
    let body = line.strip_prefix('[')?.strip_suffix(']')?.trim();
    let (section, rest) = body.split_once(char::is_whitespace)?;
    if !section.eq_ignore_ascii_case("branch") {
        return None;
    }
    parse_quoted(rest.trim())
}

fn parse_config_value(value: &str) -> String {
    if value.starts_with('"') {
        parse_quoted(value).unwrap_or_default()
    } else {
        let value = value
            .split(['#', ';'])
            .next()
            .unwrap_or_default()
            .trim_end();
        decode_config_escapes(value)
    }
}

fn decode_config_escapes(input: &str) -> String {
    let mut value = String::new();
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            value.push(match character {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{0008}',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            value.push(character);
        }
    }
    if escaped {
        value.push('\\');
    }
    value
}

fn parse_quoted(input: &str) -> Option<String> {
    let mut chars = input.chars();
    (chars.next()? == '"').then_some(())?;
    let mut value = String::new();
    let mut escaped = false;
    for character in chars {
        if escaped {
            value.push(match character {
                'n' => '\n',
                't' => '\t',
                'b' => '\u{0008}',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(value);
        } else {
            value.push(character);
        }
    }
    None
}

/// Finds the repository holding `path`, or `None` when no ancestor is a
/// working tree. A path that does not exist is in no repository.
pub fn discover(path: &Path) -> Option<Repository> {
    let start = fs::canonicalize(path).ok()?;
    let mut directory = if start.is_dir() {
        start.as_path()
    } else {
        start.parent()?
    };
    loop {
        if let Some(git_dir) = git_dir_at(directory) {
            let common_dir = common_dir_of(&git_dir);
            return Some(Repository {
                root: directory.to_path_buf(),
                git_dir,
                common_dir,
            });
        }
        directory = directory.parent()?;
    }
}

/// The git directory a `.git` entry in `directory` designates, if it holds a
/// repository. A directory is one when it carries `HEAD`; a file is a gitfile
/// naming the directory, as a linked worktree or a submodule has.
fn git_dir_at(directory: &Path) -> Option<PathBuf> {
    let dotgit = directory.join(".git");
    let metadata = fs::metadata(&dotgit).ok()?;
    let candidate = if metadata.is_dir() {
        dotgit
    } else {
        let text = fs::read_to_string(&dotgit).ok()?;
        let named = text.strip_prefix("gitdir:")?.trim();
        absolute(directory, Path::new(named))
    };
    candidate
        .join("HEAD")
        .is_file()
        .then(|| fs::canonicalize(&candidate).unwrap_or(candidate))
}

/// The shared git directory: what `commondir` names, relative to the git
/// directory, or the git directory itself when there is no such file.
fn common_dir_of(git_dir: &Path) -> PathBuf {
    let Ok(text) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let named = absolute(git_dir, Path::new(text.trim()));
    fs::canonicalize(&named).unwrap_or(named)
}

fn absolute(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(path: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(arguments)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {arguments:?} in {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// The answer git itself gives for the three facts, which is what every
    /// assertion here compares against.
    fn rev_parse(path: &Path) -> (String, String, Option<String>) {
        let root = git(path, &["rev-parse", "--show-toplevel"]);
        let common = git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        let branch = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["branch", "--show-current"])
            .output()
            .expect("git runs");
        let branch = String::from_utf8_lossy(&branch.stdout).trim().to_owned();
        (root, common, (!branch.is_empty()).then_some(branch))
    }

    fn fixture(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = crate::workspace::temp_base_outside_any_repository()
            .join(format!("hide-git-dir-{name}-{stamp}"));
        fs::create_dir_all(&path).expect("temp directory");
        path
    }

    fn repository_with_commit(root: &Path) {
        git(root, &["init", "-b", "main"]);
        fs::write(root.join("README.md"), "fixture\n").expect("fixture file");
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.email=hide@example.invalid",
                "-c",
                "user.name=hide-test",
                "commit",
                "-m",
                "fixture",
            ],
        );
    }

    #[test]
    fn reads_branch_description_with_git_quoted_section_names() {
        let root = fixture("description");
        repository_with_commit(&root);
        git(&root, &["branch", "quoted\"branch", "HEAD"]);
        git(
            &root,
            &[
                "config",
                "branch.quoted\"branch.description",
                "One-line purpose",
            ],
        );

        let repository = discover(&root).expect("repository");
        assert_eq!(
            repository
                .branch_description("quoted\"branch")
                .expect("config reads")
                .as_deref(),
            Some("One-line purpose")
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn branch_description_returns_only_the_first_logical_line() {
        let config = "[branch \"topic\"]\n\tdescription = first line\nsecond line\n";
        assert_eq!(
            parse_branch_description(config, "topic").as_deref(),
            Some("first line")
        );
    }

    #[test]
    fn branch_description_skips_bare_keys_and_decodes_git_written_values() {
        let root = fixture("description-syntax");
        repository_with_commit(&root);
        git(
            &root,
            &[
                "config",
                "branch.topic.description",
                "first \"quoted\" \\ path\nsecond line",
            ],
        );
        let config_path = root.join(".git/config");
        let written = fs::read_to_string(&config_path).expect("git config output");
        let with_bare_key = written.replace(
            "[branch \"topic\"]",
            "[branch \"topic\"]\n\tbareFlag\n\t# an unrelated comment",
        );
        fs::write(&config_path, with_bare_key).expect("augmented config");

        let repository = discover(&root).expect("repository");
        assert_eq!(
            repository
                .branch_description("topic")
                .expect("config reads")
                .as_deref(),
            Some("first \"quoted\" \\ path")
        );

        fs::remove_dir_all(root).expect("remove fixture");
    }

    fn assert_matches_git(path: &Path) {
        let (root, common, branch) = rev_parse(path);
        let found = discover(path).unwrap_or_else(|| panic!("{} is a repository", path.display()));
        assert_eq!(
            found.root,
            PathBuf::from(&root),
            "root of {}",
            path.display()
        );
        assert_eq!(
            found.common_dir,
            fs::canonicalize(&common).expect("common dir exists"),
            "common dir of {}",
            path.display()
        );
        assert_eq!(found.branch(), branch, "branch of {}", path.display());
    }

    #[test]
    fn answers_as_git_does_for_a_checkout_a_linked_worktree_and_a_nested_directory() {
        let root = fixture("repo");
        repository_with_commit(&root);
        let nested = root.join("src").join("deep");
        fs::create_dir_all(&nested).expect("nested directory");
        let worktree = root.with_file_name(format!(
            "{}-worktree",
            root.file_name().unwrap().to_string_lossy()
        ));
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "feature",
                worktree.to_str().unwrap(),
            ],
        );
        let worktree_nested = worktree.join("src");
        fs::create_dir_all(&worktree_nested).expect("worktree directory");

        for path in [&root, &nested, &worktree, &worktree_nested] {
            assert_matches_git(path);
        }
        let main = discover(&nested).unwrap();
        assert_eq!(main.main_root(), fs::canonicalize(&root).unwrap());
        let linked = discover(&worktree_nested).unwrap();
        assert_eq!(linked.root, fs::canonicalize(&worktree).unwrap());
        assert_eq!(linked.main_root(), fs::canonicalize(&root).unwrap());
        assert_eq!(linked.branch().as_deref(), Some("feature"));

        git(&worktree, &["checkout", "--detach"]);
        assert_eq!(discover(&worktree).unwrap().branch(), None);
        assert_matches_git(&worktree);
    }

    #[test]
    fn a_symlinked_path_resolves_to_the_real_checkout() {
        let root = fixture("real");
        repository_with_commit(&root);
        let link = root.with_file_name(format!(
            "{}-link",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::os::unix::fs::symlink(&root, &link).expect("symlink");
        assert_matches_git(&link);
        assert_eq!(
            discover(&link).unwrap().root,
            fs::canonicalize(&root).unwrap()
        );
    }

    #[test]
    fn a_plain_folder_a_missing_path_and_a_stray_dot_git_are_no_repository() {
        let folder = fixture("plain");
        assert_eq!(discover(&folder), None);
        assert_eq!(discover(&folder.join("missing")), None);
        // A `.git` directory with nothing in it is not a repository to git
        // either: `git rev-parse` walks past it.
        fs::create_dir_all(folder.join(".git")).expect("stray directory");
        assert_eq!(discover(&folder), None);
    }

    #[test]
    fn an_unborn_branch_is_named_and_a_file_path_belongs_to_its_directory() {
        let root = fixture("unborn");
        git(&root, &["init", "-b", "trunk"]);
        let found = discover(&root).expect("a fresh init is a repository");
        assert_eq!(found.branch().as_deref(), Some("trunk"));
        assert_eq!(found.common_dir, found.git_dir);
        fs::write(root.join("file.txt"), "x").expect("file");
        assert_eq!(
            discover(&root.join("file.txt")).unwrap().root,
            fs::canonicalize(&root).unwrap()
        );
    }
}
