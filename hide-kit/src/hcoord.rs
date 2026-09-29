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

pub(crate) fn shim_path(home: &Path) -> PathBuf {
    home.join(".hcoord").join("bin").join("hcoord")
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

pub(crate) fn observe(target: &KitTarget) -> Observed {
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
    payload::sync(&packaged(target), &copy_home(&target.home))?;
    let path = shim_path(&target.home);
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{} could not be created: {error}", folder.display()))?;
    }
    crate::write_atomically(&path, shim(&target.home, runtime).as_bytes(), 0o700)
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

/// The Node a device runs hcoord with: the one an existing hcoord shim
/// already names (the operator's choice), then the first on `PATH`, then the
/// usual install folders; each must answer a version of at least
/// [`NODE_MINIMUM`] within five seconds.
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
    candidates.extend([
        PathBuf::from("/opt/homebrew/bin/node"),
        PathBuf::from("/usr/local/bin/node"),
        home.join(".local/bin/node"),
    ]);
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
            Some(version) if version >= NODE_MINIMUM => return Ok(candidate),
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

    #[test]
    fn node_versions_compare_against_the_minimum() {
        assert_eq!(parse_node_version("v22.12.0\n"), Some((22, 12, 0)));
        assert!(parse_node_version("v22.11.9").unwrap() < NODE_MINIMUM);
        assert!(parse_node_version("v26.7.0").unwrap() >= NODE_MINIMUM);
        assert_eq!(parse_node_version("node"), None);
    }
}
