//! hcoord: the `~/.hcoord/bin/hcoord` command and the daemon it keeps.
//!
//! hcoord is a Node program. The build ships its compiled `dist/` in
//! `<kit_dir>/hcoord/`; the kit copies that to `~/.hide/kit/hcoord/` (Node
//! resolves a script's own path through links, and the daemon's LaunchAgent
//! names that path, so it must outlive the build) and writes a shim that runs
//! it with the machine's runtime. On this Mac the runtime is the app's own
//! executable in Node mode, as the desktop host ran it before; on a device it
//! is a Node the machine already has (D-27).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::{KitTarget, Observed, payload, process};

/// The oldest Node hcoord runs on (`plugins/hcoord/package.json` engines).
pub const NODE_MINIMUM: (u64, u64, u64) = (22, 12, 0);

const NODE_PROBE_DEADLINE: Duration = Duration::from_secs(5);
const ENSURE_DEADLINE: Duration = Duration::from_secs(20);

/// What runs hcoord's JavaScript on one machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HcoordRuntime {
    pub program: PathBuf,
    /// Variables the shim and the daemon need, such as Electron's Node mode
    /// or the `herdr` binary hcoord should call.
    pub env: Vec<(String, String)>,
}

/// hcoord's own relocation variable (`plugins/hcoord/src/hcoord/store.ts`,
/// `platform.ts`): every hcoord file and its daemon's LaunchAgent label follow
/// it. The kit builds its children's environment from scratch, so a
/// relocation the kit's process was started with is carried to the shim and
/// the daemon here; dropped, an isolated install would take the account's
/// default `com.hcoord.daemon` label and replace the account's own daemon.
const HOME_VARIABLE: &str = "HCOORD_HOME";

/// The hcoord relocation this process was started with, if any; empty means
/// none, as hcoord reads it.
pub(crate) fn home_override() -> Option<(String, String)> {
    std::env::var(HOME_VARIABLE)
        .ok()
        .filter(|value| !value.is_empty())
        .map(|value| (HOME_VARIABLE.to_owned(), value))
}

/// The folders under HOME that hold the shim; hcoord's own `~/.hcoord`.
const SHIM_PARTS: [&str; 2] = [".hcoord", "bin"];

pub(crate) fn shim_path(home: &Path) -> PathBuf {
    home.join(SHIM_PARTS[0]).join(SHIM_PARTS[1]).join("hcoord")
}

fn packaged(target: &KitTarget) -> PathBuf {
    target.kit_dir.join("hcoord")
}

fn copy_home(home: &Path) -> PathBuf {
    crate::record::kit_state_dir(home).join("hcoord")
}

fn cli(home: &Path) -> PathBuf {
    copy_home(home).join("dist").join("hcoord").join("cli.js")
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn shim(home: &Path, runtime: &HcoordRuntime) -> String {
    let mut line = String::new();
    for (key, value) in &runtime.env {
        line.push_str(&format!("{key}={} ", quote(value)));
    }
    format!(
        "#!/bin/sh\n{line}exec {} {} \"$@\"\n",
        quote(&runtime.program.display().to_string()),
        quote(&cli(home).display().to_string())
    )
}

/// hcoord starts its daemon through launchd only
/// (`plugins/hcoord/src/hcoord/platform.ts`), so on any other system the
/// kit has nothing it can keep running there.
fn unsupported_system(os: &str) -> Option<String> {
    (os != "macos").then(|| {
        "hcoord keeps its daemon running only on macOS, so Hide installs it on Macs".to_owned()
    })
}

pub(crate) fn observe(target: &KitTarget) -> Observed {
    if let Some(reason) = unsupported_system(std::env::consts::OS) {
        return Observed::Absent(reason);
    }
    let runtime = match &target.hcoord {
        Ok(runtime) => runtime,
        Err(reason) => return Observed::Blocked(reason.clone()),
    };
    if !packaged(target)
        .join("dist")
        .join("hcoord")
        .join("cli.js")
        .is_file()
    {
        return Observed::Blocked(format!(
            "this build has no hcoord at {}",
            packaged(target).display()
        ));
    }
    if let Err(reason) = crate::private_dirs(&target.home, &SHIM_PARTS, false)
        .and_then(|_| crate::record::private_state_dir(&target.home, false))
    {
        return Observed::Blocked(reason);
    }
    let path = shim_path(&target.home);
    let found = match std::fs::read_to_string(&path) {
        Ok(found) => found,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Observed::Missing,
        Err(error) => {
            return Observed::Blocked(format!("{} could not be read: {error}", path.display()));
        }
    };
    if found != shim(&target.home, runtime) {
        return Observed::Stale(format!("{} runs another copy of hcoord", path.display()));
    }
    match payload::is_current(&packaged(target), &copy_home(&target.home)) {
        Ok(true) => Observed::Current,
        Ok(false) => Observed::Stale("an older hcoord is installed".to_owned()),
        Err(reason) => Observed::Blocked(reason),
    }
}

pub(crate) fn install(target: &KitTarget) -> Result<(), String> {
    let runtime = target.hcoord.as_ref().map_err(Clone::clone)?;
    crate::record::private_state_dir(&target.home, true)?;
    payload::sync(&packaged(target), &copy_home(&target.home))?;
    crate::private_dirs(&target.home, &SHIM_PARTS, true)?;
    crate::write_atomically(
        &shim_path(&target.home),
        shim(&target.home, runtime).as_bytes(),
        0o700,
    )
}

/// Asks this build's hcoord to converge its daemon, as the desktop host did
/// on every launch: it replaces a daemon from another build and leaves one
/// the operator stopped stopped.
pub(crate) fn ensure(target: &KitTarget) -> Result<(), String> {
    let runtime = target.hcoord.as_ref().map_err(Clone::clone)?;
    let cli = cli(&target.home).display().to_string();
    // The daemon's LaunchAgent keeps the environment this call ran with, so
    // it names the Herdr the kit targets rather than whichever one a later
    // shell would find.
    let mut env = runtime.env.clone();
    env.push((
        "HERDR_SOCKET_PATH".to_owned(),
        target.herdr_socket.display().to_string(),
    ));
    let finished = process::run(
        &runtime.program,
        &[&cli, "daemon", "ensure", "--json"],
        &env,
        &target.home,
        ENSURE_DEADLINE,
        &target.stop,
    )?;
    let last = finished
        .stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty());
    let parsed: Option<serde_json::Value> = last.and_then(|line| serde_json::from_str(line).ok());
    match parsed {
        Some(value) if value["ok"] == true => Ok(()),
        Some(value) => Err(format!(
            "hcoord could not start its daemon: {}",
            value["error"]["message"]
                .as_str()
                .unwrap_or("no reason given")
        )),
        None => Err(format!(
            "hcoord could not start its daemon (exit {}): {}",
            finished
                .code
                .map_or("by signal".to_owned(), |code| code.to_string()),
            finished.last_error_line()
        )),
    }
}

/// The links a package manager keeps pointing at whichever Node it installed
/// last, unlike the versioned folder it installs into
/// (`/opt/homebrew/Cellar/node/<version>`), which an upgrade deletes.
fn stable_links(home: &Path) -> [PathBuf; 3] {
    [
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        home.join(".local/bin/node"),
    ]
}

/// `chosen` spelled by a stable link that resolves to the same file, so the
/// shim keeps working after the package manager upgrades that Node; a stable
/// link, or a Node no stable link leads to, is kept as it is.
fn stable_spelling(chosen: PathBuf, home: &Path) -> PathBuf {
    let links = stable_links(home);
    if links.contains(&chosen) {
        return chosen;
    }
    // `chosen` answered a version a moment ago, so this fails only if it was
    // removed since; the unchanged path is then the safe answer.
    let Ok(real) = chosen.canonicalize() else {
        return chosen;
    };
    links
        .into_iter()
        .find(|link| link.canonicalize().is_ok_and(|target| target == real))
        .unwrap_or(chosen)
}

/// The Node a device runs hcoord with: the one an existing hcoord shim
/// already names (the operator's choice), then the first on `PATH`, then the
/// usual install folders; each must answer a version of at least
/// [`NODE_MINIMUM`] within five seconds. The one found is named by a stable
/// link when one leads to it ([`stable_spelling`]).
pub fn find_node(home: &Path, stop: &AtomicBool) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Some(named) = std::fs::read_to_string(shim_path(home))
        .ok()
        .and_then(|shim| program_in_shim(&shim))
        .filter(|program| program.file_name().is_some_and(|name| name == "node"))
    {
        candidates.push(named);
    }
    if let Some(path) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path).map(|folder| folder.join("node")));
    }
    candidates.extend(stable_links(home));
    let mut seen = Vec::new();
    let mut too_old = None;
    for candidate in candidates {
        if seen.contains(&candidate) || !candidate.is_file() {
            continue;
        }
        seen.push(candidate.clone());
        let Ok(finished) = process::run(
            &candidate,
            &["--version"],
            &[],
            home,
            NODE_PROBE_DEADLINE,
            stop,
        ) else {
            continue;
        };
        match parse_node_version(&finished.stdout) {
            Some(version) if version >= NODE_MINIMUM => {
                return Ok(stable_spelling(candidate, home));
            }
            Some((major, minor, patch)) => {
                too_old.get_or_insert(format!("{major}.{minor}.{patch}"));
            }
            None => {}
        }
    }
    let (major, minor, patch) = NODE_MINIMUM;
    Err(match too_old {
        Some(found) => {
            format!("hcoord needs Node {major}.{minor}.{patch} or later; this machine has {found}")
        }
        None => format!("hcoord needs Node {major}.{minor}.{patch} or later; none was found"),
    })
}

fn parse_node_version(output: &str) -> Option<(u64, u64, u64)> {
    let mut parts = output.trim().strip_prefix('v')?.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.split(['-', '+']).next()?.parse().ok()?;
    Some((major, minor, patch))
}

/// The program a shim `exec`s, quoted with either quote or bare.
fn program_in_shim(shim: &str) -> Option<PathBuf> {
    let line = shim.lines().find(|line| line.contains("exec "))?;
    let rest = &line[line.find("exec ")? + 5..];
    let rest = rest.trim_start();
    let program = match rest.chars().next()? {
        quote @ ('\'' | '"') => {
            let body = &rest[1..];
            &body[..body.find(quote)?]
        }
        _ => rest.split_whitespace().next()?,
    };
    Some(PathBuf::from(program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shim_names_its_program_in_any_quoting() {
        assert_eq!(
            program_in_shim("#!/bin/sh\nexec \"/opt/node/bin/node\" \"/x/cli.js\" \"$@\"\n"),
            Some(PathBuf::from("/opt/node/bin/node"))
        );
        assert_eq!(
            program_in_shim("#!/bin/sh\nA='1' exec '/a b/node' '/x/cli.js' \"$@\"\n"),
            Some(PathBuf::from("/a b/node"))
        );
        assert_eq!(
            program_in_shim("#!/bin/sh\nexec /usr/bin/node /x \"$@\"\n"),
            Some(PathBuf::from("/usr/bin/node"))
        );
    }

    /// A Linux device reads hcoord as not on that machine, which offers no
    /// Reinstall, rather than as a failure no Reinstall can fix.
    #[test]
    fn hcoord_is_not_on_a_system_its_daemon_cannot_run_on() {
        assert_eq!(unsupported_system("macos"), None);
        for os in ["linux", "freebsd"] {
            assert!(
                unsupported_system(os).is_some_and(|reason| reason.contains("only on macOS")),
                "{os}"
            );
        }
    }

    fn fake_node(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\necho v26.7.0\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn shim_naming(home: &Path, program: &Path) {
        let shim = shim_path(home);
        std::fs::create_dir_all(shim.parent().unwrap()).unwrap();
        std::fs::write(
            &shim,
            format!(
                "#!/bin/sh\nexec '{}' '/x/cli.js' \"$@\"\n",
                program.display()
            ),
        )
        .unwrap();
    }

    /// A shim naming the versioned folder a package manager installed Node
    /// into is rewritten to the link that keeps leading to Node after an
    /// upgrade deletes that folder; a Node no such link leads to stays named.
    #[test]
    fn a_node_reached_through_a_stable_link_is_named_by_that_link() {
        let stop = AtomicBool::new(false);

        let home = tempfile::tempdir().unwrap();
        let versioned = home.path().join("Cellar/node/26.7.0/bin/node");
        fake_node(&versioned);
        let link = home.path().join(".local/bin/node");
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&versioned, &link).unwrap();
        shim_naming(home.path(), &versioned);
        assert_eq!(find_node(home.path(), &stop), Ok(link.clone()));
        // The rewritten shim is a fixed point: the next pass keeps it.
        shim_naming(home.path(), &link);
        assert_eq!(find_node(home.path(), &stop), Ok(link.clone()));

        // A stable link is kept as spelled, never swapped for another stable
        // link that reaches the same file.
        assert_eq!(stable_spelling(link.clone(), home.path()), link);

        let other = tempfile::tempdir().unwrap();
        let chosen = other.path().join("tools/node/bin/node");
        fake_node(&chosen);
        shim_naming(other.path(), &chosen);
        assert_eq!(find_node(other.path(), &stop), Ok(chosen));
    }

    #[test]
    fn node_versions_compare_against_the_minimum() {
        assert_eq!(parse_node_version("v22.12.0\n"), Some((22, 12, 0)));
        assert!(parse_node_version("v22.11.9").unwrap() < NODE_MINIMUM);
        assert!(parse_node_version("v26.7.0").unwrap() >= NODE_MINIMUM);
        assert_eq!(parse_node_version("node"), None);
    }
}
