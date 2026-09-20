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
    InvalidGitLink {
        path: PathBuf,
        reason: String,
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
            Self::InvalidGitLink { path, reason } => {
                write!(
                    formatter,
                    "project_git_link_invalid:{}:{reason}",
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
    let (root, kind) = match git::discover_checked(&canonical)? {
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
    use super::ResolveError;
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
        discover_checked(path).ok().flatten()
    }

    pub fn discover_checked(path: &Path) -> Result<Option<Repository>, ResolveError> {
        let start = fs::canonicalize(path).map_err(|error| ResolveError::InvalidGitLink {
            path: path.to_path_buf(),
            reason: format!("discovery_path:{error}"),
        })?;
        let mut directory = if start.is_dir() {
            start.as_path()
        } else {
            start.parent().ok_or_else(|| ResolveError::InvalidGitLink {
                path: start.clone(),
                reason: "path_has_no_parent".to_owned(),
            })?
        };
        loop {
            if let Some((git_dir, common_dir)) = git_dir_at(directory)? {
                return Ok(Some(Repository {
                    root: directory.to_path_buf(),
                    git_dir,
                    common_dir,
                }));
            }
            let Some(parent) = directory.parent() else {
                return Ok(None);
            };
            directory = parent;
        }
    }

    fn git_dir_at(directory: &Path) -> Result<Option<(PathBuf, PathBuf)>, ResolveError> {
        let dotgit = directory.join(".git");
        let metadata = match fs::symlink_metadata(&dotgit) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(invalid(&dotgit, format!("metadata:{error}"))),
        };
        if metadata.file_type().is_symlink() {
            return Err(invalid(&dotgit, "dotgit_symlink"));
        }
        if metadata.is_dir() {
            let candidate = canonical(&dotgit, "git_directory")?;
            require_regular_file(&candidate.join("HEAD"), "head")?;
            return Ok(Some((candidate.clone(), candidate)));
        }
        if !metadata.is_file() {
            return Err(invalid(&dotgit, "dotgit_not_file_or_directory"));
        }

        let text = fs::read_to_string(&dotgit)
            .map_err(|error| invalid(&dotgit, format!("pointer_read:{error}")))?;
        let named = text
            .trim()
            .strip_prefix("gitdir:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| invalid(&dotgit, "pointer_format"))?;
        let candidate = canonical(&absolute(directory, Path::new(named)), "git_directory")?;
        require_regular_file(&candidate.join("HEAD"), "head")?;
        let common_dir = common_dir_of(&candidate)?;

        let candidate_parent = candidate
            .parent()
            .ok_or_else(|| invalid(&candidate, "worktree_entry_has_no_parent"))?;
        if canonical(candidate_parent, "worktrees_directory")?
            != canonical(
                &common_dir.join("worktrees"),
                "registered_worktrees_directory",
            )?
        {
            return Err(invalid(&dotgit, "unregistered_worktree_entry"));
        }

        let reciprocal = candidate.join("gitdir");
        require_regular_file(&reciprocal, "worktree_gitdir")?;
        let reciprocal_text = fs::read_to_string(&reciprocal)
            .map_err(|error| invalid(&reciprocal, format!("worktree_gitdir_read:{error}")))?;
        let reciprocal_target = canonical(
            &absolute(&candidate, Path::new(reciprocal_text.trim())),
            "worktree_checkout_pointer",
        )?;
        if reciprocal_target != canonical(&directory.join(".git"), "checkout_pointer")? {
            return Err(invalid(&reciprocal, "worktree_pointer_not_reciprocal"));
        }
        Ok(Some((candidate, common_dir)))
    }

    fn common_dir_of(git_dir: &Path) -> Result<PathBuf, ResolveError> {
        let path = git_dir.join("commondir");
        require_regular_file(&path, "commondir")?;
        let text = fs::read_to_string(&path)
            .map_err(|error| invalid(&path, format!("commondir_read:{error}")))?;
        canonical(
            &absolute(git_dir, Path::new(text.trim())),
            "common_directory",
        )
    }

    fn canonical(path: &Path, label: &str) -> Result<PathBuf, ResolveError> {
        fs::canonicalize(path).map_err(|error| invalid(path, format!("{label}:{error}")))
    }

    fn require_regular_file(path: &Path, label: &str) -> Result<(), ResolveError> {
        let metadata = fs::symlink_metadata(path)
            .map_err(|error| invalid(path, format!("{label}_metadata:{error}")))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(invalid(path, format!("{label}_not_regular_file")));
        }
        Ok(())
    }

    fn invalid(path: &Path, reason: impl Into<String>) -> ResolveError {
        ResolveError::InvalidGitLink {
            path: path.to_path_buf(),
            reason: reason.into(),
        }
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

    #[cfg(unix)]
    #[test]
    fn dotgit_symlink_is_rejected_instead_of_aliasing_another_project() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        let attacker = temp.path().join("attacker");
        fs::create_dir(&victim).unwrap();
        fs::create_dir(&attacker).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&victim)
                .status()
                .unwrap()
                .success()
        );
        symlink(victim.join(".git"), attacker.join(".git")).unwrap();

        assert!(matches!(
            resolve(&attacker, "local"),
            Err(ResolveError::InvalidGitLink { reason, .. }) if reason == "dotgit_symlink"
        ));
    }

    #[test]
    fn forged_git_pointer_without_reciprocal_worktree_registration_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let victim = temp.path().join("victim");
        let attacker = temp.path().join("attacker");
        fs::create_dir(&victim).unwrap();
        fs::create_dir(&attacker).unwrap();
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&victim)
                .status()
                .unwrap()
                .success()
        );
        fs::write(
            attacker.join(".git"),
            format!("gitdir: {}\n", victim.join(".git").display()),
        )
        .unwrap();

        assert!(matches!(
            resolve(&attacker, "local"),
            Err(ResolveError::InvalidGitLink { reason, .. })
                if reason.contains("commondir_metadata")
        ));
    }
}
