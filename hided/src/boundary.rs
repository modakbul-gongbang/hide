//! The `$HOME` filesystem boundary the web shell's registration flow lives
//! behind (PRD web-shell-pivot-s2, D-09 and B10).
//!
//! The web shell only draws what this module answers: a directory listing for
//! its path autocomplete and a refusal with a reason code. Both are decided on
//! the real path (`canonicalize`), so a symlink that leaves home, a `..`
//! segment, or an encoded segment that happens to exist all resolve before the
//! containment test runs. The boundary root is read from `HOME` at boot and is
//! not configurable (`practices/env.md`); an allowed-roots setting is an S5
//! candidate.
//!
//! Nothing here reaches the core: a refused path is answered to the client and
//! logged, and an accepted path is forwarded as the canonical path that was
//! checked, so the core registers exactly what the boundary saw.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Why a path was not answered or forwarded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Refusal {
    /// The real path is not under the home directory.
    OutsideHome,
    /// The path is the home directory itself; a workspace is a subdirectory.
    HomeRoot,
    /// Nothing exists at the path.
    NotFound,
    /// The path exists but is not a directory.
    NotADirectory,
    /// Empty, relative, or otherwise not a path this daemon reads.
    InvalidPath,
}

impl Refusal {
    /// The reason code the contract names (`contracts/hided-ws.schema.json`).
    pub fn code(self) -> &'static str {
        match self {
            Self::OutsideHome => "outside_home",
            Self::HomeRoot => "home_root",
            Self::NotFound => "not_found",
            Self::NotADirectory => "not_a_directory",
            Self::InvalidPath => "invalid_path",
        }
    }
}

/// One directory the listing shows: its name and the path a client sends back.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
}

/// A listing answer: the directory that was listed and its visible children.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Listing {
    pub root_path: String,
    pub entries: Vec<Entry>,
    /// More than `LIST_CAP` children existed; the rest were not read.
    pub truncated: bool,
}

/// Children a listing carries at most; a home directory with more subfolders
/// than this is answered as truncated, not grown (engineering principle 15).
pub const LIST_CAP: usize = 500;

#[derive(Clone, Debug)]
pub struct Boundary {
    home: PathBuf,
}

impl Boundary {
    /// Reads the boundary root once. Fails when `$HOME` does not resolve to a
    /// directory: a daemon without a boundary must not serve the registration
    /// flow at all.
    pub fn new(home: &Path) -> Result<Self, String> {
        let real = home
            .canonicalize()
            .map_err(|error| format!("HOME {} does not resolve: {error}", home.display()))?;
        if !real.is_dir() {
            return Err(format!("HOME {} is not a directory", home.display()));
        }
        Ok(Self { home: real })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The canonical directory for `raw` when it is home or under home.
    pub fn resolve_dir(&self, raw: &str) -> Result<PathBuf, Refusal> {
        let path = Path::new(raw);
        if raw.is_empty() || raw.contains('\0') || !path.is_absolute() {
            return Err(Refusal::InvalidPath);
        }
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            // `..` would resolve, but a path that names its way out of a
            // directory is not one the input field produces; refusing the
            // shape keeps the log readable when it appears.
            return Err(Refusal::InvalidPath);
        }
        let real = match path.canonicalize() {
            Ok(real) => real,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(Refusal::NotFound);
            }
            Err(error) if error.kind() == io::ErrorKind::NotADirectory => {
                return Err(Refusal::NotADirectory);
            }
            Err(_) => return Err(Refusal::InvalidPath),
        };
        if !real.starts_with(&self.home) {
            return Err(Refusal::OutsideHome);
        }
        let metadata = fs::metadata(&real).map_err(|_| Refusal::NotFound)?;
        if !metadata.is_dir() {
            return Err(Refusal::NotADirectory);
        }
        Ok(real)
    }

    /// The canonical path a `create_workspace` may carry: a directory strictly
    /// under home.
    pub fn resolve_workspace(&self, raw: &str) -> Result<PathBuf, Refusal> {
        let real = self.resolve_dir(raw)?;
        if real == self.home {
            return Err(Refusal::HomeRoot);
        }
        Ok(real)
    }

    /// The visible subdirectories of `raw`: no files, no hidden names, and no
    /// symlink whose target leaves home or is not a directory.
    pub fn list(&self, raw: &str) -> Result<Listing, Refusal> {
        let root = self.resolve_dir(raw)?;
        let read = fs::read_dir(&root).map_err(|_| Refusal::NotFound)?;
        let mut entries = Vec::new();
        let mut truncated = false;
        for item in read {
            let Ok(item) = item else { continue };
            let name = item.file_name();
            let Some(name) = name.to_str() else { continue };
            if name.starts_with('.') {
                continue;
            }
            let Ok(real) = item.path().canonicalize() else {
                continue;
            };
            if !real.starts_with(&self.home) || !real.is_dir() {
                continue;
            }
            if entries.len() == LIST_CAP {
                truncated = true;
                break;
            }
            entries.push(Entry {
                name: name.to_owned(),
                path: root.join(name).display().to_string(),
            });
        }
        entries.sort_by_key(|entry| entry.name.to_lowercase());
        Ok(Listing {
            root_path: root.display().to_string(),
            entries,
            truncated,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Fixture {
        _outer: tempfile::TempDir,
        home: PathBuf,
        outside: PathBuf,
        boundary: Boundary,
    }

    fn fixture() -> Fixture {
        let outer = tempfile::tempdir().unwrap();
        let home = outer.path().join("home");
        let outside = outer.path().join("outside");
        fs::create_dir_all(home.join("projects/alpha")).unwrap();
        fs::create_dir_all(home.join("projects/Beta")).unwrap();
        fs::create_dir_all(home.join("projects/.hidden")).unwrap();
        fs::create_dir_all(outside.join("secret")).unwrap();
        fs::write(home.join("projects/notes.txt"), "x").unwrap();
        symlink(&outside, home.join("projects/escape")).unwrap();
        symlink(home.join("projects/alpha"), home.join("projects/alias")).unwrap();
        symlink(
            home.join("projects/notes.txt"),
            home.join("projects/filelink"),
        )
        .unwrap();
        let boundary = Boundary::new(&home).unwrap();
        Fixture {
            _outer: outer,
            home: boundary.home().to_path_buf(),
            outside,
            boundary,
        }
    }

    fn s(path: &Path) -> String {
        path.display().to_string()
    }

    #[test]
    fn lists_only_visible_directories_inside_home() {
        let f = fixture();
        let listing = f.boundary.list(&s(&f.home.join("projects"))).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alias", "alpha", "Beta"]);
        assert!(!listing.truncated);
        assert_eq!(listing.root_path, s(&f.home.join("projects")));
    }

    #[test]
    fn a_symlink_out_of_home_is_refused_even_when_named_directly() {
        let f = fixture();
        let escape = s(&f.home.join("projects/escape"));
        assert_eq!(f.boundary.list(&escape), Err(Refusal::OutsideHome));
        assert_eq!(
            f.boundary.resolve_workspace(&escape),
            Err(Refusal::OutsideHome)
        );
        let deeper = s(&f.home.join("projects/escape/secret"));
        assert_eq!(
            f.boundary.resolve_workspace(&deeper),
            Err(Refusal::OutsideHome)
        );
    }

    #[test]
    fn parent_segments_and_encoded_segments_do_not_leave_home() {
        let f = fixture();
        let dotdot = format!("{}/projects/../../outside", s(&f.home));
        assert_eq!(
            f.boundary.resolve_workspace(&dotdot),
            Err(Refusal::InvalidPath)
        );
        let encoded = format!("{}/projects/%2e%2e/%2e%2e/outside", s(&f.home));
        assert_eq!(
            f.boundary.resolve_workspace(&encoded),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            f.boundary.resolve_workspace(&s(&f.outside)),
            Err(Refusal::OutsideHome)
        );
        let sibling_prefix = format!("{}-extra", s(&f.home));
        fs::create_dir_all(&sibling_prefix).unwrap();
        assert_eq!(
            f.boundary.resolve_workspace(&sibling_prefix),
            Err(Refusal::OutsideHome),
            "a sibling whose name starts with the home path is outside"
        );
    }

    #[test]
    fn files_hidden_dirs_home_itself_and_bad_shapes() {
        let f = fixture();
        let file = s(&f.home.join("projects/notes.txt"));
        assert_eq!(
            f.boundary.resolve_workspace(&file),
            Err(Refusal::NotADirectory)
        );
        assert_eq!(f.boundary.list(&file), Err(Refusal::NotADirectory));
        let filelink = s(&f.home.join("projects/filelink"));
        assert_eq!(
            f.boundary.resolve_workspace(&filelink),
            Err(Refusal::NotADirectory)
        );
        let under_file = s(&f.home.join("projects/notes.txt/child"));
        assert!(matches!(
            f.boundary.resolve_workspace(&under_file),
            Err(Refusal::NotFound | Refusal::NotADirectory)
        ));
        let missing = s(&f.home.join("projects/nope"));
        assert_eq!(
            f.boundary.resolve_workspace(&missing),
            Err(Refusal::NotFound)
        );
        assert_eq!(
            f.boundary.resolve_workspace(&s(&f.home)),
            Err(Refusal::HomeRoot)
        );
        assert!(f.boundary.list(&s(&f.home)).is_ok(), "home itself lists");
        assert_eq!(f.boundary.resolve_workspace(""), Err(Refusal::InvalidPath));
        assert_eq!(
            f.boundary.resolve_workspace("projects"),
            Err(Refusal::InvalidPath)
        );
        assert_eq!(
            f.boundary
                .resolve_workspace(&format!("{}\0", s(&f.home.join("projects")))),
            Err(Refusal::InvalidPath)
        );
        // A hidden directory is hidden from the listing but may be typed.
        let hidden = s(&f.home.join("projects/.hidden"));
        assert!(f.boundary.resolve_workspace(&hidden).is_ok());
    }

    #[test]
    fn an_accepted_path_is_the_canonical_one() {
        let f = fixture();
        let alias = s(&f.home.join("projects/alias"));
        assert_eq!(
            f.boundary.resolve_workspace(&alias).unwrap(),
            f.home.join("projects/alpha")
        );
        let trailing = format!("{}/", s(&f.home.join("projects/alpha")));
        assert_eq!(
            f.boundary.resolve_workspace(&trailing).unwrap(),
            f.home.join("projects/alpha")
        );
    }

    #[test]
    fn the_listing_is_capped() {
        let outer = tempfile::tempdir().unwrap();
        let home = outer.path().join("home");
        for i in 0..(LIST_CAP + 3) {
            fs::create_dir_all(home.join(format!("d{i:04}"))).unwrap();
        }
        let boundary = Boundary::new(&home).unwrap();
        let listing = boundary.list(&s(boundary.home())).unwrap();
        assert_eq!(listing.entries.len(), LIST_CAP);
        assert!(listing.truncated);
    }

    #[test]
    fn a_missing_home_refuses_to_boot() {
        assert!(Boundary::new(Path::new("/nonexistent/home/for/hided")).is_err());
    }
}
