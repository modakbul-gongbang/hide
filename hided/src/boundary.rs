//! The `$HOME` filesystem boundary the web shell's registration flow lives
//! behind (PRD web-shell-pivot-s2, D-09 and B10).
//!
//! The web shell only draws what this module answers: a directory listing for
//! its path autocomplete and a refusal with a reason code. A path is tested
//! twice: as written, before the filesystem is touched, so a refusal for a
//! path outside home carries one reason (`outside_home`) whether or not the
//! path exists; then on its real path (`canonicalize`), so a symlink under home
//! that leaves it is caught too. `..` and encoded segments are refused on
//! shape or resolve like any other name. The boundary root is read from `HOME`
//! at boot and is not configurable (`practices/env.md`); an allowed-roots
//! setting is an S5 candidate.
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
    /// The real home directory every accepted path resolves under.
    home: PathBuf,
    /// Home as `HOME` names it; a client writes paths under this spelling
    /// when home itself sits behind a symlink (`/var` for `/private/var`).
    home_as_given: PathBuf,
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
        Ok(Self {
            home: real,
            home_as_given: home.to_path_buf(),
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The canonical directory for `raw` when it is home or under home.
    /// `~` and `~/...` name the home directory, so a client can start its
    /// listing without knowing the path; the answer carries the real one.
    /// A path written outside home is refused before the filesystem is read,
    /// so the reason never says whether such a path exists.
    pub fn resolve_dir(&self, raw: &str) -> Result<PathBuf, Refusal> {
        let expanded;
        let raw = if raw == "~" || raw.starts_with("~/") {
            expanded = self
                .home
                .join(raw.trim_start_matches('~').trim_start_matches('/'));
            expanded.to_string_lossy().into_owned()
        } else {
            raw.to_owned()
        };
        let path = Path::new(&raw);
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
        if !path.starts_with(&self.home) && !path.starts_with(&self.home_as_given) {
            return Err(Refusal::OutsideHome);
        }
        let real = match path.canonicalize() {
            Ok(real) => real,
            Err(error) => {
                // Where the path stopped resolving decides the reason: past
                // a symlink that already left home, nothing deeper is
                // described, so the reason cannot probe the rest of the disk.
                if !self.deepest_existing_ancestor_is_inside(path) {
                    return Err(Refusal::OutsideHome);
                }
                return Err(match error.kind() {
                    io::ErrorKind::NotFound => Refusal::NotFound,
                    io::ErrorKind::NotADirectory => Refusal::NotADirectory,
                    _ => Refusal::InvalidPath,
                });
            }
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

    /// Whether the nearest ancestor of `path` that resolves lies under home
    /// by real path. `/` always resolves, so a path that was written under
    /// home always has one.
    fn deepest_existing_ancestor_is_inside(&self, path: &Path) -> bool {
        path.ancestors()
            .skip(1)
            .find_map(|ancestor| ancestor.canonicalize().ok())
            .is_some_and(|real| real.starts_with(&self.home))
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
        // Past the escape the reason is the same whether the rest exists,
        // is a file, or is missing: the disk beyond home is not described.
        fs::write(f.outside.join("secret/marker.txt"), "x").unwrap();
        for tail in [
            "nope",
            "secret/nope",
            "secret/marker.txt",
            "secret/marker.txt/child",
        ] {
            let path = s(&f.home.join("projects/escape").join(tail));
            assert_eq!(
                f.boundary.resolve_workspace(&path),
                Err(Refusal::OutsideHome),
                "{tail}"
            );
            assert_eq!(f.boundary.list(&path), Err(Refusal::OutsideHome), "{tail}");
        }
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
    fn a_path_outside_home_gets_one_reason_whether_or_not_it_exists() {
        let f = fixture();
        let existing_dir = s(&f.outside.join("secret"));
        let missing_dir = s(&f.outside.join("nope"));
        let existing_file = s(&f.outside.join("secret/marker.txt"));
        fs::write(&existing_file, "x").unwrap();
        for path in [
            &existing_dir,
            &missing_dir,
            &existing_file,
            &"/etc/nope-zzz".to_owned(),
        ] {
            assert_eq!(
                f.boundary.resolve_workspace(path),
                Err(Refusal::OutsideHome),
                "{path}: the reason must not tell whether it exists"
            );
            assert_eq!(f.boundary.list(path), Err(Refusal::OutsideHome), "{path}");
        }
        // A symlink into home from outside is still outside: the path as
        // written decides, not where it leads.
        let inbound = f.outside.join("inbound");
        symlink(f.home.join("projects/alpha"), &inbound).unwrap();
        assert_eq!(
            f.boundary.resolve_workspace(&s(&inbound)),
            Err(Refusal::OutsideHome)
        );
    }

    #[test]
    fn home_spelled_as_given_resolves_when_home_is_a_symlink() {
        let outer = tempfile::tempdir().unwrap();
        let real_home = outer.path().join("real-home");
        fs::create_dir_all(real_home.join("projects/alpha")).unwrap();
        let link_home = outer.path().join("link-home");
        symlink(&real_home, &link_home).unwrap();
        let boundary = Boundary::new(&link_home).unwrap();
        let canonical = real_home.canonicalize().unwrap();
        assert_eq!(boundary.home(), canonical);
        assert_eq!(
            boundary.resolve_workspace(&s(&link_home.join("projects/alpha"))),
            Ok(canonical.join("projects/alpha")),
            "the spelling HOME uses is under home"
        );
        assert_eq!(
            boundary.resolve_workspace(&s(&canonical.join("projects/alpha"))),
            Ok(canonical.join("projects/alpha"))
        );
        assert_eq!(
            boundary.list(&s(&link_home)).unwrap().root_path,
            s(&canonical)
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
        assert_eq!(f.boundary.list("~").unwrap().root_path, s(&f.home));
        assert_eq!(
            f.boundary.resolve_workspace("~/projects/alpha").unwrap(),
            f.home.join("projects/alpha")
        );
        assert_eq!(f.boundary.resolve_workspace("~"), Err(Refusal::HomeRoot));
        assert_eq!(
            f.boundary.resolve_workspace("~user/x"),
            Err(Refusal::InvalidPath)
        );
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
