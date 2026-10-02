//! hcoord: the `~/.hide/hcoord/bin/hcoord` command, its `hcoord` link on
//! `PATH`, and the daemon it keeps.
//!
//! hcoord is a Node program. The build ships its compiled `dist/` in
//! `<kit_dir>/hcoord/`; the kit copies that to `~/.hide/kit/hcoord/` (Node
//! resolves a script's own path through links, and the daemon's LaunchAgent
//! names that path, so it must outlive the build) and writes a shim in
//! hcoord's home that runs it with the machine's runtime. On this Mac the
//! runtime is the app's own executable in Node mode, as the desktop host ran
//! it before; on a device it is a Node the machine already has (D-27).
//!
//! hcoord's home moved from `~/.hcoord` to `~/.hide/hcoord` (PRD
//! hide-home-layout D-08, D-09). While the old home is there the part reads
//! as outdated, and its install first asks this build's hcoord to adopt it,
//! before the copy changes the code the old daemon's LaunchAgent runs; a
//! relocated hcoord (HCOORD_HOME) is never moved.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use crate::layout;
use crate::{KitTarget, Observed, payload, process};

/// The oldest Node hcoord runs on (`plugins/hcoord/package.json` engines).
pub const NODE_MINIMUM: (u64, u64, u64) = (22, 12, 0);

const NODE_PROBE_DEADLINE: Duration = Duration::from_secs(5);
const ENSURE_DEADLINE: Duration = Duration::from_secs(20);
/// Adopting the old home waits for launchd to let go of the old daemon,
/// which launchd gives 20 s before it kills one that ignores SIGTERM.
const ADOPT_DEADLINE: Duration = Duration::from_secs(60);

/// The name of the link on `PATH` (D-02).
const LINK_NAME: &str = "hcoord";

/// What runs hcoord's JavaScript on one machine.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HcoordRuntime {
    pub program: PathBuf,
    /// Variables the shim and the daemon need, such as Electron's Node mode,
    /// the `herdr` binary hcoord should call, or hcoord's relocation. The
    /// kit builds its children's environment from scratch, so a relocation
    /// dropped here would give an isolated install the account's default
    /// LaunchAgent label.
    pub env: Vec<(String, String)>,
}

/// The relocation as the runtime carries it to the shim and the daemon.
pub(crate) fn relocation_env(relocated: Option<&Path>) -> Option<(String, String)> {
    relocated.map(|home| {
        (
            layout::HCOORD_HOME_VARIABLE.to_owned(),
            home.display().to_string(),
        )
    })
}

fn home_dir(target: &KitTarget) -> PathBuf {
    layout::hcoord_home(&target.home, target.hcoord_home.as_deref())
}

pub(crate) fn shim_path(target: &KitTarget) -> PathBuf {
    layout::hcoord_command(&home_dir(target))
}

/// The folders down to the shim's, each checked by `crate::private_dirs`:
/// from HOME for the default home, so `~/.hide` is checked too.
fn shim_dirs(target: &KitTarget, create: bool) -> Result<PathBuf, String> {
    match &target.hcoord_home {
        None => crate::private_dirs(&target.home, &[layout::HIDE_HOME, "hcoord", "bin"], create),
        Some(relocated) => {
            let parent = relocated
                .parent()
                .ok_or_else(|| format!("{} has no parent folder", relocated.display()))?;
            let name = relocated
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| format!("{} is not a folder name", relocated.display()))?;
            crate::private_dirs(parent, &[name, "bin"], create)
        }
    }
}

/// The old home still waiting to be adopted: there, hcoord not relocated,
/// and no new home yet. With both there nothing is moved and the row says so
/// (`note`), so a stray old home never blocks an update of the new one.
fn legacy_home(target: &KitTarget) -> Option<PathBuf> {
    let legacy = left_legacy_home(target)?;
    std::fs::symlink_metadata(home_dir(target))
        .is_err()
        .then_some(legacy)
}

/// The old home when hcoord is not relocated and it is still there, whether
/// or not the new home exists too.
fn left_legacy_home(target: &KitTarget) -> Option<PathBuf> {
    if target.hcoord_home.is_some() {
        return None;
    }
    let legacy = layout::legacy_hcoord_home(&target.home);
    std::fs::symlink_metadata(&legacy).is_ok().then_some(legacy)
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
    if let Err(reason) =
        shim_dirs(target, false).and_then(|_| crate::record::private_state_dir(&target.home, false))
    {
        return Observed::Blocked(reason);
    }
    // Before the shim: a part the kit recorded and whose new shim is not
    // there yet would otherwise read as taken away and never move.
    if let Some(legacy) = legacy_home(target) {
        return Observed::Stale(format!(
            "hcoord is still in {}; Hide moves it to {}",
            legacy.display(),
            home_dir(target).display()
        ));
    }
    let path = shim_path(target);
    let found = match std::fs::read_to_string(&path) {
        Ok(found) => found,
        // A home with a ledger and no shim is one a move filled and an
        // install did not finish (removal never takes hcoord's home), so it
        // is finished on the next pass rather than read as taken away.
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && home_dir(target).join("ledger.json").is_file() =>
        {
            return Observed::Stale(format!(
                "hcoord moved to {} but is not installed there yet",
                home_dir(target).display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Observed::Missing,
        Err(error) => {
            return Observed::Blocked(format!("{} could not be read: {error}", path.display()));
        }
    };
    if found != shim(&target.home, runtime) {
        return Observed::Stale(format!("{} runs another copy of hcoord", path.display()));
    }
    match payload::is_current(&packaged(target), &copy_home(&target.home)) {
        Ok(true) => {}
        Ok(false) => return Observed::Stale("an older hcoord is installed".to_owned()),
        Err(reason) => return Observed::Blocked(reason),
    }
    // A relocated hcoord is not the account's, so `hcoord` on PATH is not
    // its to take.
    if target.hcoord_home.is_some() {
        return Observed::Current;
    }
    match link_state(target) {
        Link::Current | Link::Foreign(_) => Observed::Current,
        Link::Missing | Link::Ours => Observed::Stale(format!(
            "{} does not lead to hcoord yet",
            link_path(target).display()
        )),
    }
}

pub(crate) fn install(target: &KitTarget) -> Result<(), String> {
    let runtime = target.hcoord.as_ref().map_err(Clone::clone)?;
    crate::record::private_state_dir(&target.home, true)?;
    if let Some(legacy) = legacy_home(target) {
        adopt(target, runtime, &legacy)?;
    }
    payload::sync(&packaged(target), &copy_home(&target.home))?;
    shim_dirs(target, true)?;
    crate::write_atomically(
        &shim_path(target),
        shim(&target.home, runtime).as_bytes(),
        0o700,
    )?;
    // A name another program holds is left and reported on the row; hcoord
    // itself is installed either way. A relocated hcoord never takes it.
    if target.hcoord_home.is_none() && matches!(link_state(target), Link::Missing | Link::Ours) {
        link(target)?;
    }
    Ok(())
}

/// Asks this build's packaged hcoord to adopt the old home: it stops the old
/// daemon, renames the folder whole and drops only the junk, or leaves the
/// old home and its daemon as they were (`plugins/hcoord/src/hcoord/home.ts`).
/// The packaged copy runs, not the installed one: the installed copy is the
/// code the old daemon's LaunchAgent runs, and it must not change until the
/// home it reads has moved.
fn adopt(target: &KitTarget, runtime: &HcoordRuntime, legacy: &Path) -> Result<(), String> {
    let packaged_cli = packaged(target).join("dist").join("hcoord").join("cli.js");
    // hcoord derives the old home from the HOME it runs with, which is
    // `target.home`; no path is passed, so the command moves nothing else.
    let finished = process::run(
        &runtime.program,
        &[
            &packaged_cli.display().to_string(),
            "home",
            "adopt",
            "--json",
        ],
        &runtime.env,
        &target.home,
        ADOPT_DEADLINE,
        &target.stop,
    )?;
    let parsed: Option<serde_json::Value> = finished
        .stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str(line).ok());
    let reason = match parsed {
        Some(value) if value["ok"] == true => return Ok(()),
        Some(value) => value["error"]["message"]
            .as_str()
            .unwrap_or("no reason given")
            .to_owned(),
        None => format!(
            "exit {}: {}",
            finished
                .code
                .map_or("by signal".to_owned(), |code| code.to_string()),
            finished.last_error_line()
        ),
    };
    Err(format!(
        "hcoord could not move {} to {}: {reason}. The old hcoord keeps running, and the next launch or Reinstall tries again",
        legacy.display(),
        home_dir(target).display()
    ))
}

pub(crate) fn link_path(target: &KitTarget) -> PathBuf {
    target.cli_dir.join(LINK_NAME)
}

enum Link {
    Current,
    Missing,
    /// Hide's link to an older place: the old home's shim or anywhere under
    /// `~/.hide`.
    Ours,
    /// Another program's file or link; the sentence says what is there.
    Foreign(String),
}

fn link_state(target: &KitTarget) -> Link {
    let path = link_path(target);
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Link::Missing,
        Err(error) => {
            return Link::Foreign(format!(
                "{} could not be inspected: {error}",
                path.display()
            ));
        }
    };
    if !metadata.file_type().is_symlink() {
        return Link::Foreign(format!(
            "{} is another program's file; Hide left it",
            path.display()
        ));
    }
    match std::fs::read_link(&path) {
        Ok(destination) if destination == shim_path(target) => Link::Current,
        Ok(destination)
            if !destination
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
                && (destination
                    == layout::hcoord_command(&layout::legacy_hcoord_home(&target.home))
                    || destination.starts_with(layout::hide_home(&target.home))) =>
        {
            Link::Ours
        }
        Ok(destination) => Link::Foreign(format!(
            "{} already points at {}; Hide left it",
            path.display(),
            destination.display()
        )),
        Err(error) => Link::Foreign(format!("{} could not be read: {error}", path.display())),
    }
}

/// Why `hcoord` on `PATH` is not Hide's, for an installed hcoord row (B14).
/// What an installed hcoord row adds below its location, which the row
/// already shows: an old home left beside the new one (never merged), and
/// another program's `hcoord` on PATH.
pub(crate) fn note(target: &KitTarget) -> Option<String> {
    let mut notes = Vec::new();
    if let Some(legacy) = left_legacy_home(target) {
        notes.push(format!(
            "{} is still there beside {}; Hide never merges two hcoord homes, so it left both and runs the new one. Move the old one away when nothing in it is needed",
            legacy.display(),
            home_dir(target).display()
        ));
    }
    if target.hcoord_home.is_none() {
        if let Link::Foreign(reason) = link_state(target) {
            notes.push(format!(
                "{reason}, so `hcoord` on PATH does not run this copy"
            ));
        }
    }
    (!notes.is_empty()).then(|| notes.join(". "))
}

fn link(target: &KitTarget) -> Result<(), String> {
    let path = link_path(target);
    std::fs::create_dir_all(&target.cli_dir)
        .map_err(|error| format!("{} could not be created: {error}", target.cli_dir.display()))?;
    let temporary = target
        .cli_dir
        .join(format!(".{LINK_NAME}.hide-kit-{}", std::process::id()));
    let _ = std::fs::remove_file(&temporary);
    std::os::unix::fs::symlink(shim_path(target), &temporary)
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            format!("{} could not be linked: {error}", path.display())
        })
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

/// `chosen` spelled by one of `links` that resolves to the same file, so the
/// shim keeps working after the package manager upgrades that Node; one of
/// `links`, or a Node none of them leads to, is kept as it is.
fn stable_spelling(chosen: PathBuf, links: &[PathBuf]) -> PathBuf {
    if links.contains(&chosen) {
        return chosen;
    }
    // `chosen` answered a version a moment ago, so this fails only if it was
    // removed since; the unchanged path is then the safe answer.
    let Ok(real) = chosen.canonicalize() else {
        return chosen;
    };
    links
        .iter()
        .find(|link| link.canonicalize().is_ok_and(|target| target == real))
        .cloned()
        .unwrap_or(chosen)
}

/// The Node a device runs hcoord with: the one an existing hcoord shim
/// already names (the operator's choice), in hcoord's home or else in the
/// old `~/.hcoord` it has not moved from yet, then the first on `PATH`, then
/// the usual install folders; each must answer a version of at least
/// [`NODE_MINIMUM`] within five seconds. The one found is named by a stable
/// link when one leads to it ([`stable_spelling`]).
pub fn find_node(home: &Path, hcoord_home: &Path, stop: &AtomicBool) -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    for shim in [
        layout::hcoord_command(hcoord_home),
        layout::hcoord_command(&layout::legacy_hcoord_home(home)),
    ] {
        if let Some(named) = std::fs::read_to_string(shim)
            .ok()
            .and_then(|shim| program_in_shim(&shim))
            .filter(|program| program.file_name().is_some_and(|name| name == "node"))
        {
            candidates.push(named);
        }
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
                return Ok(stable_spelling(candidate, &stable_links(home)));
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
        let shim = layout::hcoord_command(&layout::default_hcoord_home(home));
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
        let hcoord_home = layout::default_hcoord_home(home.path());
        assert_eq!(
            find_node(home.path(), &hcoord_home, &stop),
            Ok(link.clone())
        );
        // The rewritten shim is a fixed point: the next pass keeps it.
        shim_naming(home.path(), &link);
        assert_eq!(
            find_node(home.path(), &hcoord_home, &stop),
            Ok(link.clone())
        );

        // A stable link is kept as spelled, never swapped for an earlier
        // stable link that reaches the same file.
        let earlier = home.path().join("opt/bin/node");
        std::fs::create_dir_all(earlier.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&versioned, &earlier).unwrap();
        let links = [earlier.clone(), link.clone()];
        assert_eq!(stable_spelling(link.clone(), &links), link);
        assert_eq!(stable_spelling(versioned.clone(), &links), earlier);

        let other = tempfile::tempdir().unwrap();
        let chosen = other.path().join("tools/node/bin/node");
        fake_node(&chosen);
        shim_naming(other.path(), &chosen);
        assert_eq!(
            find_node(
                other.path(),
                &layout::default_hcoord_home(other.path()),
                &stop
            ),
            Ok(chosen)
        );
    }

    /// A device that has not moved hcoord yet keeps the Node its old shim
    /// named, so the move does not swap the operator's Node for another.
    #[test]
    fn the_old_homes_shim_still_names_the_node() {
        let stop = AtomicBool::new(false);
        let home = tempfile::tempdir().unwrap();
        let chosen = home.path().join("tools/node/bin/node");
        fake_node(&chosen);
        let old = layout::hcoord_command(&layout::legacy_hcoord_home(home.path()));
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(
            &old,
            format!(
                "#!/bin/sh\nexec '{}' '/x/cli.js' \"$@\"\n",
                chosen.display()
            ),
        )
        .unwrap();
        assert_eq!(
            find_node(
                home.path(),
                &layout::default_hcoord_home(home.path()),
                &stop
            ),
            Ok(chosen)
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
