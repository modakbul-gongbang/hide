//! The device's install kit, run by the helper for the core (PRD
//! device-parity D-09, D-10, D-16).
//!
//! The core uploads every part of a build into `<root>/<version>/`, the
//! folder this helper runs from. Before it installs, the helper points
//! `<root>/current` at that folder, so the hooks, the `hide` link and the
//! plugin name a path that outlives the build (B16), and removes the builds
//! the link no longer leads to. The root is always the one the running helper
//! was installed under; no request can name another folder. A helper under
//! the default root also retires the old layout's root and bridge folder
//! once nothing names them (`hide_kit::legacy::device`), whose paths are
//! constants of the helper, never a request's.
//!
//! When the SSH channel closes, [`stop`] raises the kit's stop flag, which
//! ends a child the kit is waiting on, so the helper does not outlive its
//! connection by more than one bounded step (engineering rule 14).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};

use serde_json::Value;

use crate::error::{ErrorCode, HostError, HostResult};
use crate::protocol::{KitAction, KitRemoved};
use crate::serve::{Env, KitPlace};

static STOP: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(false)));

/// The helper's input ended: a kit step still running stops.
pub(crate) fn stop() {
    STOP.store(true, Ordering::Relaxed);
}

/// The flag [`stop`] raises, which a helper's [`Env`] carries.
pub(crate) fn process_stop() -> Arc<AtomicBool> {
    Arc::clone(&STOP)
}

pub use hide_kit::is_build_name;

/// Answers one `kit` request for the node `env` describes: the helper root
/// the running helper was installed under, or the desktop package the core's
/// own node runs from.
pub fn handle(
    action: KitAction,
    cli_dir: &str,
    herdr_socket: Option<&str>,
    retirement_projects: &[String],
    env: &Env,
) -> HostResult<Value> {
    match &env.kit {
        KitPlace::Installed => {}
        KitPlace::Bundled(kit_dir) => {
            return bundled(
                kit_dir,
                env,
                action,
                cli_dir,
                herdr_socket,
                retirement_projects,
            );
        }
        KitPlace::Standalone => {
            return Err(HostError::new(
                ErrorCode::Unsupported,
                hide_kit::STANDALONE_REASON,
            ));
        }
    }
    let home = env.home.as_deref();
    let executable = std::env::current_exe().map_err(|error| {
        HostError::new(
            ErrorCode::Io,
            format!("The helper could not tell where it runs from: {error}"),
        )
    })?;
    let (root, version) = install_layout(&executable)?;
    let home = home.map(Path::to_path_buf).ok_or_else(|| {
        HostError::new(
            ErrorCode::Unsupported,
            "The home folder is unknown, so the helper cannot tell where to install",
        )
    })?;
    run_for_projects(
        &Placement {
            root,
            version,
            home,
        },
        action,
        cli_dir,
        herdr_socket,
        retirement_projects,
        Arc::clone(&env.stop),
    )
}

/// The kit of the machine whose desktop package holds the parts: they stay
/// where the package put them, so nothing is pointed or pruned, and the
/// machine itself is never removed from Hide.
fn bundled(
    kit_dir: &Path,
    env: &Env,
    action: KitAction,
    cli_dir: &str,
    herdr_socket: Option<&str>,
    retirement_projects: &[String],
) -> HostResult<Value> {
    let home = env.home.clone().ok_or_else(|| {
        HostError::new(
            ErrorCode::Unsupported,
            "HOME is not set, so Hide cannot tell where to install",
        )
    })?;
    let inputs = Inputs::read(&home, cli_dir, herdr_socket, retirement_projects)?;
    let mut target =
        hide_kit::local_target(kit_dir, &home, &inputs.herdr_socket, Arc::clone(&env.stop));
    target.cli_dir = inputs.cli_dir;
    target.retirement_projects = inputs.projects;
    match action {
        KitAction::Apply | KitAction::Reinstall { .. } => {
            to_value(hide_kit::apply(&target, &scope_of(action)))
        }
        KitAction::Status => to_value(hide_kit::status(&target)),
        KitAction::Remove => Err(HostError::new(
            ErrorCode::Unsupported,
            "The core's own machine is not removed from Hide",
        )),
    }
}

/// Where the running helper was installed.
#[derive(Debug)]
pub struct Placement {
    pub root: PathBuf,
    pub version: String,
    pub home: PathBuf,
}

/// The helper root and build folder of a helper at
/// `<root>/<16 hex>/hided`; any other copy, such as one a
/// developer started by hand, runs no kit.
fn install_layout(executable: &Path) -> HostResult<(PathBuf, String)> {
    let refused = || {
        HostError::new(
            ErrorCode::Unsupported,
            format!(
                "{} is not a helper Hide installed, so it does not install the kit",
                executable.display()
            ),
        )
    };
    let version_dir = executable.parent().ok_or_else(refused)?;
    let version = version_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_build_name(name))
        .ok_or_else(refused)?;
    let root = version_dir.parent().ok_or_else(refused)?;
    Ok((root.to_path_buf(), version.to_owned()))
}

/// Runs `action` for a helper placed at `placement`. Public for the core's
/// tests, which place a helper folder without a device.
pub fn run(
    placement: &Placement,
    action: KitAction,
    cli_dir: &str,
    herdr_socket: Option<&str>,
    stop: Arc<AtomicBool>,
) -> HostResult<Value> {
    run_for_projects(placement, action, cli_dir, herdr_socket, &[], stop)
}

fn run_for_projects(
    placement: &Placement,
    action: KitAction,
    cli_dir: &str,
    herdr_socket: Option<&str>,
    retirement_projects: &[String],
    stop: Arc<AtomicBool>,
) -> HostResult<Value> {
    let home = &placement.home;
    let Inputs {
        projects,
        cli_dir,
        herdr_socket,
    } = Inputs::read(home, cli_dir, herdr_socket, retirement_projects)?;
    let target = || {
        let mut target = hide_kit::device_target(
            &placement.root,
            home,
            &cli_dir,
            &herdr_socket,
            Arc::clone(&stop),
        );
        target.retirement_projects = projects.clone();
        target
    };
    match action {
        KitAction::Apply | KitAction::Reinstall { .. } => {
            let preflight_target = target();
            if let Err(reason) = hide_kit::retirement_preflight(&preflight_target) {
                return to_value(hide_kit::retirement_blocked(&preflight_target, reason));
            }
            point_current(&placement.root, &placement.version)?;
            remove_other_builds(&placement.root, &placement.version);
            to_value(hide_kit::apply(&target(), &scope_of(action)))
        }
        KitAction::Status => to_value(hide_kit::status(&target())),
        KitAction::Remove => {
            let kit = hide_kit::remove(&target());
            to_value(KitRemoved {
                kit,
                helper_root: remove_root(&placement.root),
            })
        }
    }
}

/// The paths a `kit` request names, expanded under the node's home.
struct Inputs {
    projects: Vec<PathBuf>,
    cli_dir: PathBuf,
    herdr_socket: PathBuf,
}

impl Inputs {
    fn read(
        home: &Path,
        cli_dir: &str,
        herdr_socket: Option<&str>,
        retirement_projects: &[String],
    ) -> HostResult<Self> {
        let projects = retirement_projects
            .iter()
            .map(|project| expand(project, home))
            .collect::<HostResult<Vec<_>>>()?;
        let cli_dir = expand(cli_dir, home)?;
        let herdr_socket = match herdr_socket {
            Some(socket) => expand(socket, home)?,
            // Herdr's default for this home, in Herdr's own order.
            None => {
                let variables = |name: &str| {
                    if name == hide_platform::host::HOME_VARIABLE {
                        Some(home.as_os_str().to_owned())
                    } else {
                        std::env::var_os(name)
                    }
                };
                hide_platform::host::herdr_socket_default_from(&variables).map_err(|error| {
                    HostError::new(
                        ErrorCode::Unsupported,
                        format!("Herdr's default socket has no location: {error}"),
                    )
                })?
            }
        };
        Ok(Self {
            projects,
            cli_dir,
            herdr_socket,
        })
    }
}

/// What an `apply` or `reinstall` asks the kit to do.
fn scope_of(action: KitAction) -> hide_kit::Scope {
    match action {
        KitAction::Reinstall {
            components,
            agents_on,
            agents_off,
            codex_daemon_off,
        } => {
            let scope = hide_kit::Scope::reinstall(components).merge(hide_kit::Scope::agents(
                agents_on.iter().map(String::as_str),
                agents_off.iter().map(String::as_str),
            ));
            if codex_daemon_off {
                scope.merge(hide_kit::Scope::codex_daemon_off())
            } else {
                scope
            }
        }
        _ => hide_kit::Scope::automatic(),
    }
}

fn to_value(value: impl serde::Serialize) -> HostResult<Value> {
    serde_json::to_value(value).map_err(|error| {
        HostError::new(
            ErrorCode::Io,
            format!("The kit answer could not be encoded: {error}"),
        )
    })
}

/// `~/rest` under `home`, or an absolute path as it is.
fn expand(path: &str, home: &Path) -> HostResult<PathBuf> {
    let expanded = match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        // The core's own node on Windows names its own folders natively.
        None if path.starts_with('/') || Path::new(path).is_absolute() => PathBuf::from(path),
        None => {
            return Err(HostError::new(
                ErrorCode::InvalidPath,
                format!("{path:?} must be absolute or start with ~/"),
            ));
        }
    };
    if expanded
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(HostError::new(
            ErrorCode::InvalidPath,
            format!("{path:?} is not a plain path"),
        ));
    }
    Ok(expanded)
}

/// Points `<root>/current` at `version` through a link made beside it and
/// renamed over it, so the name is never missing in between. On Linux a
/// program started through `current` at that moment can, rarely, be told it
/// does not exist (see `replace_link`).
fn point_current(root: &Path, version: &str) -> HostResult<()> {
    let current = root.join(hide_kit::CURRENT);
    if hide_platform::fs::link::is_link_to(&current, Path::new(version)) {
        return Ok(());
    }
    hide_platform::fs::link::replace_link(Path::new(version), &current).map_err(|error| {
        HostError::new(
            ErrorCode::Io,
            format!(
                "{} could not be pointed at {version}: {error}",
                current.display()
            ),
        )
    })
}

/// The build folders under `root` other than `keep`: only names the core's
/// install creates.
fn builds(root: &Path, keep: Option<&str>) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| is_build_name(name) && Some(name) != keep)
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
        .map(|entry| entry.path())
        .collect()
}

/// Removes the builds `current` no longer leads to. A build another
/// connection still runs keeps running: its files stay open until it exits.
fn remove_other_builds(root: &Path, keep: &str) {
    for build in builds(root, Some(keep)) {
        let _ = std::fs::remove_dir_all(build);
    }
}

/// Removes every build, the `current` link and then the root itself, which
/// is removed only when nothing else is left in it: the root is the folder
/// the operator allowed Hide to own, but a file someone else put there is not
/// Hide's to delete.
fn remove_root(root: &Path) -> hide_kit::RemoveOutcome {
    let mut failures = Vec::new();
    for build in builds(root, None) {
        if let Err(error) = std::fs::remove_dir_all(&build) {
            failures.push(format!("{} stayed: {error}", build.display()));
        }
    }
    let current = root.join(hide_kit::CURRENT);
    if std::fs::symlink_metadata(&current).is_ok_and(|meta| meta.file_type().is_symlink())
        && let Err(error) = hide_platform::fs::link::remove_link(&current)
    {
        failures.push(format!("{} stayed: {error}", current.display()));
    }
    if !failures.is_empty() {
        return hide_kit::RemoveOutcome::Failed {
            reason: failures.join("; "),
        };
    }
    match std::fs::remove_dir(root) {
        Ok(()) => hide_kit::RemoveOutcome::Removed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            hide_kit::RemoveOutcome::Absent
        }
        Err(_) => hide_kit::RemoveOutcome::Kept {
            reason: format!(
                "{} holds files Hide did not put there, so it stays",
                root.display()
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placed(dir: &Path, version: &str) -> Placement {
        let root = dir.join("host-helper");
        std::fs::create_dir_all(root.join(version)).unwrap();
        std::fs::write(root.join(version).join("hided"), b"helper").unwrap();
        Placement {
            root,
            version: version.to_owned(),
            home: dir.join("home"),
        }
    }

    #[test]
    fn only_a_helper_in_a_build_folder_runs_the_kit() {
        let (root, version) = install_layout(Path::new(
            "/home/me/.local/share/hide/host-helper/0123456789abcdef/hided",
        ))
        .unwrap();
        assert_eq!(
            root,
            PathBuf::from("/home/me/.local/share/hide/host-helper")
        );
        assert_eq!(version, "0123456789abcdef");
        for elsewhere in [
            "/work/hide/target/debug/hided",
            "/home/me/.local/share/hide/host-helper/current/hided",
            "/Applications/hide.app/Contents/Resources/hided",
        ] {
            let refused = install_layout(Path::new(elsewhere)).unwrap_err();
            assert_eq!(refused.code, ErrorCode::Unsupported, "{elsewhere}");
        }
    }

    #[test]
    fn an_install_points_current_at_the_running_build_and_drops_older_ones() {
        let dir = tempfile::tempdir().unwrap();
        let older = placed(dir.path(), "aaaaaaaaaaaaaaaa");
        let placement = placed(dir.path(), "bbbbbbbbbbbbbbbb");
        std::fs::write(placement.root.join("notes.txt"), b"mine").unwrap();
        hide_platform::fs::link::create_link(
            Path::new(&older.version),
            &placement.root.join("current"),
        )
        .unwrap();

        point_current(&placement.root, &placement.version).unwrap();
        remove_other_builds(&placement.root, &placement.version);

        assert!(hide_platform::fs::link::is_link_to(
            &placement.root.join("current"),
            Path::new("bbbbbbbbbbbbbbbb")
        ));
        assert!(!placement.root.join(&older.version).exists());
        assert!(placement.root.join("current/hided").is_file());
        // Only build folders are Hide's to remove.
        assert!(placement.root.join("notes.txt").is_file());
    }

    #[test]
    fn blocked_retirement_does_not_repoint_current_or_remove_a_build() {
        let directory = tempfile::tempdir().unwrap();
        let older = placed(directory.path(), "aaaaaaaaaaaaaaaa");
        let placement = placed(directory.path(), "bbbbbbbbbbbbbbbb");
        hide_platform::fs::link::create_link(
            Path::new(&older.version),
            &placement.root.join("current"),
        )
        .unwrap();
        let ledger = placement.home.join(".hide/hcoord/ledger.json");
        std::fs::create_dir_all(ledger.parent().unwrap()).unwrap();
        let bytes = serde_json::json!({"schema":"hcoord.ledger.v1","requests":{"open":{"status":"open"}},"watches":{}}).to_string();
        std::fs::write(&ledger, &bytes).unwrap();
        let report = run(
            &placement,
            KitAction::Apply,
            "~/.local/bin",
            Some("~/private-herdr.sock"),
            Arc::default(),
        )
        .unwrap();
        let report: hide_kit::KitReport = serde_json::from_value(report).unwrap();
        assert_eq!(
            report
                .component(hide_kit::ComponentId::CoordinationRetirement)
                .unwrap()
                .state,
            hide_kit::ComponentState::Failed
        );
        assert!(hide_platform::fs::link::is_link_to(
            &placement.root.join("current"),
            Path::new(&older.version)
        ));
        assert!(placement.root.join(&older.version).is_dir());
        assert_eq!(std::fs::read_to_string(ledger).unwrap(), bytes);
        assert!(!placement.home.join(".hide/kit").exists());
        assert!(!placement.home.join(".local/bin").exists());
    }

    #[test]
    fn removal_takes_the_root_only_when_nothing_else_is_in_it() {
        let dir = tempfile::tempdir().unwrap();
        let placement = placed(dir.path(), "bbbbbbbbbbbbbbbb");
        point_current(&placement.root, &placement.version).unwrap();
        std::fs::write(placement.root.join("notes.txt"), b"mine").unwrap();

        let kept = remove_root(&placement.root);
        assert!(
            matches!(kept, hide_kit::RemoveOutcome::Kept { .. }),
            "{kept:?}"
        );
        assert!(!placement.root.join("bbbbbbbbbbbbbbbb").exists());
        assert!(placement.root.join("notes.txt").is_file());

        std::fs::remove_file(placement.root.join("notes.txt")).unwrap();
        assert_eq!(
            remove_root(&placement.root),
            hide_kit::RemoveOutcome::Removed
        );
        assert!(!placement.root.exists());
    }

    #[test]
    fn a_request_path_outside_home_notation_is_refused() {
        let home = Path::new("/home/me");
        assert_eq!(
            expand("~/.local/bin", home).unwrap(),
            home.join(".local/bin")
        );
        assert_eq!(expand("/opt/bin", home).unwrap(), PathBuf::from("/opt/bin"));
        for refused in ["bin", "~/../other", "/opt/../etc"] {
            assert_eq!(
                expand(refused, home).unwrap_err().code,
                ErrorCode::InvalidPath,
                "{refused}"
            );
        }
    }
}
