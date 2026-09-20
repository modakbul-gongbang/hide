//! One durable project identity shared by the catalog, sessions, memory, and hooks.
//!
//! Git linked worktrees resolve to the main worktree. Plain folders retain their
//! canonical path. A failed resolution is an error, never permission to search a
//! display name or a neighbouring project.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectIdentity {
    pub id: String,
    pub root: PathBuf,
    pub device_id: String,
    pub kind: ProjectKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectKind {
    Git,
    Folder,
}

#[derive(Debug)]
pub enum ResolveError {
    EmptyDevice,
    MissingPath(PathBuf),
    Canonicalize {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl Display for ResolveError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyDevice => formatter.write_str("project_device_empty"),
            Self::MissingPath(path) => write!(formatter, "project_path_missing:{}", path.display()),
            Self::Canonicalize { path, source } => {
                write!(
                    formatter,
                    "project_path_unreadable:{}:{source}",
                    path.display()
                )
            }
        }
    }
}

impl Error for ResolveError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Canonicalize { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn resolve(path: &Path, device_id: &str) -> Result<ProjectIdentity, ResolveError> {
    if device_id.trim().is_empty() {
        return Err(ResolveError::EmptyDevice);
    }
    if !path.exists() {
        return Err(ResolveError::MissingPath(path.to_path_buf()));
    }
    let canonical = fs::canonicalize(path).map_err(|source| ResolveError::Canonicalize {
        path: path.to_path_buf(),
        source,
    })?;
    let (root, kind) = match git::discover(&canonical) {
        Some(repository) => (repository.main_root(), ProjectKind::Git),
        None => (
            if canonical.is_dir() {
                canonical
            } else {
                canonical
                    .parent()
                    .map(Path::to_path_buf)
                    .ok_or_else(|| ResolveError::MissingPath(path.to_path_buf()))?
            },
            ProjectKind::Folder,
        ),
    };
    let material = format!("{}\0{}", device_id, root.to_string_lossy());
    let digest = Sha256::digest(material.as_bytes());
    Ok(ProjectIdentity {
        id: format!("project:{:x}", digest),
        root,
        device_id: device_id.to_owned(),
        kind,
    })
}

pub mod git {
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Repository {
        pub root: PathBuf,
        pub git_dir: PathBuf,
        pub common_dir: PathBuf,
    }

    impl Repository {
        pub fn main_root(&self) -> PathBuf {
            self.common_dir
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone())
        }

        pub fn branch(&self) -> Option<String> {
            let head = fs::read_to_string(self.git_dir.join("HEAD")).ok()?;
            let reference = head.trim().strip_prefix("ref:")?.trim();
            let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
            (!name.is_empty()).then(|| name.to_owned())
        }

        pub fn branch_description(&self, branch: &str) -> Result<Option<String>, String> {
            let path = self.common_dir.join("config");
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("repository config could not be read: {error}"))?;
            Ok(parse_branch_description(&text, branch))
        }

        pub fn branch_issue(&self, branch: &str) -> Result<Option<String>, String> {
            let path = self.common_dir.join("config");
            let text = fs::read_to_string(&path)
                .map_err(|error| format!("repository config could not be read: {error}"))?;
            Ok(parse_branch_value(&text, branch, "issue"))
        }
    }

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

    fn parse_branch_description(config: &str, wanted_branch: &str) -> Option<String> {
        parse_branch_value(config, wanted_branch, "description")
    }

    fn parse_branch_value(
        config: &str,
        wanted_branch: &str,
        wanted_key: &str,
    ) -> Option<String> {
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
                continue;
            };
            if !key.trim().eq_ignore_ascii_case(wanted_key) {
                continue;
            }
            let value = parse_config_value(value.trim());
            let first = value.lines().next().unwrap_or_default().trim_end();
            return (!first.is_empty()).then(|| first.to_owned());
        }
        None
    }

    fn parse_branch_section(line: &str) -> Option<String> {
        let body = line.strip_prefix('[')?.strip_suffix(']')?.trim();
        let (section, rest) = body.split_once(char::is_whitespace)?;
        section.eq_ignore_ascii_case("branch").then_some(())?;
        parse_quoted(rest.trim())
    }

    fn parse_config_value(value: &str) -> String {
        if value.starts_with('"') {
            parse_quoted(value).unwrap_or_default()
        } else {
            decode_config_escapes(
                value
                    .split(['#', ';'])
                    .next()
                    .unwrap_or_default()
                    .trim_end(),
            )
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

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn branch_issue_reads_the_first_config_line() {
            let config = concat!(
                "[branch \"topic\"]\n",
                "\tissue = \"owner/repository#42\"\n",
                "\tdescription = purpose\n",
            );

            assert_eq!(
                parse_branch_value(config, "topic", "issue").as_deref(),
                Some("owner/repository#42")
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn linked_worktrees_share_one_project_identity() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("main");
        let linked = temp.path().join("linked");
        fs::create_dir(&main).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        fs::write(main.join("README.md"), "test\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "."])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-qm", "init"])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    "linked",
                    linked.to_str().unwrap()
                ])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        let main_id = resolve(&main, "local").unwrap();
        let linked_id = resolve(&linked, "local").unwrap();
        assert_eq!(main_id.id, linked_id.id);
        assert_eq!(main_id.root, linked_id.root);
        assert_eq!(linked_id.kind, ProjectKind::Git);
    }

    #[test]
    fn device_is_part_of_identity_and_missing_paths_never_fallback() {
        let temp = tempfile::tempdir().unwrap();
        assert_ne!(
            resolve(temp.path(), "local").unwrap().id,
            resolve(temp.path(), "mini").unwrap().id
        );
        assert!(matches!(
            resolve(&temp.path().join("missing"), "local"),
            Err(ResolveError::MissingPath(_))
        ));
    }
}
