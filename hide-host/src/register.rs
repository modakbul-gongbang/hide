//! Whether a folder may be registered as a project on the machine that holds
//! it (PRD S5.5 B24).
//!
//! The rule is the one hided applies to this machine's registrations
//! (`hided/src/boundary.rs`, `resolve_workspace`): the real path is a
//! directory strictly inside the home directory. A device judges it with its
//! own home, so registering there never widens what a registration may name;
//! the project the folder belongs to must be inside that home too, because
//! the registration names the project, not the folder asked for.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, HostError, HostResult};

/// The project a registrable folder belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registrable {
    /// The project's root: the main worktree of its repository, or the
    /// folder itself.
    pub root: String,
    pub is_git: bool,
}

pub fn check(path: &Path, home: &Path) -> HostResult<Registrable> {
    let home = home.canonicalize().map_err(|error| {
        HostError::new(
            ErrorCode::Io,
            format!("The home folder {} cannot be read: {error}", home.display()),
        )
    })?;
    let real = path.canonicalize().map_err(|_| {
        HostError::new(
            ErrorCode::NotFound,
            format!("{} does not exist", path.display()),
        )
    })?;
    if !real.is_dir() {
        return Err(HostError::new(
            ErrorCode::NotADirectory,
            format!("{} is not a folder", real.display()),
        ));
    }
    let outside = |place: &Path| {
        if place == home {
            HostError::new(
                ErrorCode::OutsideRoot,
                format!(
                    "The home folder {} cannot be a project; choose a folder inside it",
                    home.display()
                ),
            )
        } else {
            HostError::new(
                ErrorCode::OutsideRoot,
                format!(
                    "Only folders inside the home folder {} can be added",
                    home.display()
                ),
            )
        }
    };
    if real == home || !real.starts_with(&home) {
        return Err(outside(&real));
    }
    let facts = hide_project::facts(&real)
        .map_err(|error| HostError::new(ErrorCode::Io, error.to_string()))?;
    if facts.root == home || !facts.root.starts_with(&home) {
        return Err(HostError::new(
            ErrorCode::OutsideRoot,
            format!(
                "{} belongs to {}, which is not a folder inside the home folder {}",
                real.display(),
                facts.root.display(),
                home.display()
            ),
        ));
    }
    Ok(Registrable {
        root: facts.root.to_string_lossy().into_owned(),
        is_git: facts.kind == hide_project::ProjectKind::Git,
    })
}
