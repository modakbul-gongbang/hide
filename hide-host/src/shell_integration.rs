//! Hide's shell integration: the startup files that let a pane's shell say
//! when it has finished starting, after the user's own startup files and
//! anything they asked at the keyboard (a `[Y/n]` update question), so Hide
//! types an agent's command only then.
//!
//! It is VS Code's terminal shell integration, reached through the
//! environment alone, because Herdr starts a pane's shell itself and takes
//! only an `env` from Hide: zsh reads its startup files from `ZDOTDIR`, and
//! fish reads the `vendor_conf.d` of each folder in `XDG_DATA_DIRS`. Each file
//! here reads the user's own files where and in the order the shell would,
//! and gives both variables back as the user had them, unset included, which
//! the environment carries in `HIDE_SHELL_ZDOTDIR` and
//! `HIDE_SHELL_XDG_DATA_DIRS`. The first prompt reports a Herdr pane token,
//! `hide_prompt`, which is the mark Hide reads (Herdr does not parse OSC 133
//! prompt marks). bash, sh, PowerShell and cmd take their startup file only
//! as an argument, so for an account whose shell is one of those nothing is
//! added to the environment, and a start there is confirmed after the fact
//! instead (`herdr-core`'s `agent_start`).
//!
//! The files live under the node's account home
//! (`hide_kit::layout::shell_integration`), one folder per version of their
//! text, written once and never rewritten. A shell reads them only while it
//! starts; the first prompt has given the environment back, so nothing in the
//! pane, or anything it starts later, refers to them again. Another build's
//! folder is removed once no pane has been started with it for an hour.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::time::{Duration, SystemTime};

/// The token the first prompt reports on the pane, and the value it reports.
pub const PROMPT_TOKEN: &str = "hide_prompt";

#[cfg(unix)]
const ZSH: [(&str, &str); 4] = [
    (".zshenv", include_str!("shell_integration/zshenv.zsh")),
    (".zprofile", include_str!("shell_integration/zprofile.zsh")),
    (".zshrc", include_str!("shell_integration/zshrc.zsh")),
    (".zlogin", include_str!("shell_integration/zlogin.zsh")),
];
#[cfg(unix)]
const FISH: &str = include_str!("shell_integration/hide.fish");
/// Where fish finds the file under the data folder Hide puts first.
#[cfg(unix)]
const FISH_FILE: &str = "fish/vendor_conf.d/hide.fish";
/// The data folders a program reads when `XDG_DATA_DIRS` is unset (XDG Base
/// Directory specification), which the fish file leaves in place.
#[cfg(unix)]
const XDG_DATA_DIRS_DEFAULT: &str = "/usr/local/share:/usr/share";
/// The shells whose startup files the environment reaches.
#[cfg(unix)]
const SHELLS: [&str; 2] = ["zsh", "fish"];
/// How long another build's folder stays after a pane was last started
/// with it. A shell reads its later startup files only once a question in
/// an earlier one is answered, which can be long after a start stopped
/// waiting for it (`herdr-core`'s `agent_start::SHELL_WAIT`, 30 s), and one
/// that finds the folder gone starts without the user's own files.
#[cfg(unix)]
const UNUSED_FOR: Duration = Duration::from_secs(60 * 60);

/// What the node's account starts a pane's shell with, which Hide's values
/// replace: the shell, and the user's own `ZDOTDIR` and `XDG_DATA_DIRS`,
/// `None` where unset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserShell {
    pub shell: Option<String>,
    pub zdotdir: Option<String>,
    pub data_dirs: Option<String>,
}

impl UserShell {
    /// The node's own process environment, the closest it knows to the one
    /// Herdr gives a pane.
    pub fn of_process() -> Self {
        Self {
            shell: std::env::var("SHELL").ok(),
            zdotdir: std::env::var("ZDOTDIR").ok(),
            data_dirs: std::env::var("XDG_DATA_DIRS").ok(),
        }
    }
}

/// Writes the integration under `home` once and answers the environment a
/// pane's shell is started with. Empty when `user`'s shell is not one the
/// environment reaches, and on Windows, where no shell can take it: nothing
/// of Hide's then reaches the pane.
#[cfg(unix)]
pub fn install(home: &Path, user: &UserShell) -> io::Result<BTreeMap<String, String>> {
    let reached = user
        .shell
        .as_deref()
        .and_then(|shell| Path::new(shell).file_name())
        .and_then(|name| name.to_str())
        .is_some_and(|name| SHELLS.contains(&name));
    if !reached {
        return Ok(BTreeMap::new());
    }
    let folder = version_folder(home)?;
    let with = |path: PathBuf| path.to_string_lossy().into_owned();
    let inherited = user
        .data_dirs
        .as_deref()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or(XDG_DATA_DIRS_DEFAULT);
    let mut env = BTreeMap::from([
        ("ZDOTDIR".to_owned(), with(folder.join("zsh"))),
        (
            "XDG_DATA_DIRS".to_owned(),
            format!("{}:{inherited}", with(folder.join("fish-data"))),
        ),
    ]);
    // What the files give back; absent stays absent.
    for (name, value) in [
        ("HIDE_SHELL_ZDOTDIR", &user.zdotdir),
        ("HIDE_SHELL_XDG_DATA_DIRS", &user.data_dirs),
    ] {
        if let Some(value) = value {
            env.insert(name.to_owned(), value.clone());
        }
    }
    Ok(env)
}

#[cfg(windows)]
pub fn install(_home: &Path, _user: &UserShell) -> io::Result<BTreeMap<String, String>> {
    Ok(BTreeMap::new())
}

/// The folder of this build's files, written whole when it is missing.
#[cfg(unix)]
fn version_folder(home: &Path) -> io::Result<PathBuf> {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for (name, text) in ZSH.iter().chain(&[(FISH_FILE, FISH)]) {
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(text.as_bytes());
        digest.update([0]);
    }
    let root = hide_kit::layout::shell_integration(home);
    let folder = root.join(&format!("{:x}", digest.finalize())[..16]);
    if folder.is_dir() {
        // Started with now: another build's sweep keeps it.
        std::fs::File::open(&folder)?.set_modified(SystemTime::now())?;
        remove_unused(&root, &folder);
        return Ok(folder);
    }
    hide_platform::fs::private::create_dir_all(&root)?;
    // Written beside its name and moved there in one rename, so a shell never
    // reads a folder half written; another writer that got there first wrote
    // the same text.
    let staging = tempfile::Builder::new()
        .prefix(".staging-")
        .tempdir_in(&root)?;
    let written = (|| {
        let zsh = staging.path().join("zsh");
        std::fs::create_dir(&zsh)?;
        for (name, text) in ZSH {
            std::fs::write(zsh.join(name), text)?;
        }
        let fish = staging.path().join("fish-data").join(FISH_FILE);
        std::fs::create_dir_all(
            fish.parent()
                .ok_or_else(|| io::Error::other("no fish folder"))?,
        )?;
        std::fs::write(&fish, FISH)
    })();
    // A writer of the same text may have finished first.
    if let Err(error) = written {
        return if folder.is_dir() {
            Ok(folder)
        } else {
            Err(error)
        };
    }
    match hide_platform::fs::atomic::rename_no_replace_path(staging.path(), &folder) {
        Ok(()) => {
            // The folder now lives at its name; nothing is left to remove.
            let _ = staging.keep();
            remove_unused(&root, &folder);
            Ok(folder)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && folder.is_dir() => Ok(folder),
        Err(error) => Err(error),
    }
}

/// Removes every other version, and what an interrupted write left, once no
/// pane has been started with it for [`UNUSED_FOR`]: the folder holds this
/// build's version and those of the builds that started panes within it.
#[cfg(unix)]
fn remove_unused(root: &Path, kept: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let unused = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > UNUSED_FOR);
        if path != kept && unused {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg(all(test, unix))]
mod real_shell_tests;

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn zsh_user() -> UserShell {
        UserShell {
            shell: Some("/bin/zsh".into()),
            ..Default::default()
        }
    }

    #[test]
    fn the_environment_names_files_that_read_the_users_own_first() {
        let home = tempfile::tempdir().unwrap();
        let env = install(home.path(), &zsh_user()).unwrap();
        let zsh = Path::new(&env["ZDOTDIR"]);
        for (name, _) in ZSH {
            assert!(zsh.join(name).is_file(), "{name}");
        }
        let (data, rest) = env["XDG_DATA_DIRS"].split_once(':').unwrap();
        assert_eq!(rest, XDG_DATA_DIRS_DEFAULT);
        assert!(Path::new(data).join(FISH_FILE).is_file());
        assert!(zsh.starts_with(hide_kit::layout::shell_integration(home.path())));
        assert!(!env.contains_key("HIDE_SHELL_ZDOTDIR"), "the user had none");
        assert!(!env.contains_key("HIDE_SHELL_XDG_DATA_DIRS"));
        // The user's own values follow, and a second install answers the
        // same folder without writing it again.
        let user = UserShell {
            shell: Some("/opt/homebrew/bin/fish".into()),
            zdotdir: Some("/home/me/.config/zsh".into()),
            data_dirs: Some("/opt/share".into()),
        };
        let again = install(home.path(), &user).unwrap();
        assert_eq!(again["ZDOTDIR"], env["ZDOTDIR"]);
        assert_eq!(again["XDG_DATA_DIRS"], format!("{data}:/opt/share"));
        assert_eq!(again["HIDE_SHELL_ZDOTDIR"], "/home/me/.config/zsh");
        assert_eq!(again["HIDE_SHELL_XDG_DATA_DIRS"], "/opt/share");
        let entries = std::fs::read_dir(hide_kit::layout::shell_integration(home.path()))
            .unwrap()
            .count();
        assert_eq!(entries, 1, "no staging folder is left behind");
    }

    #[test]
    fn a_shell_the_environment_does_not_reach_gets_nothing_of_hides() {
        let home = tempfile::tempdir().unwrap();
        for shell in [Some("/bin/bash"), Some("/bin/sh"), None] {
            let user = UserShell {
                shell: shell.map(str::to_owned),
                ..Default::default()
            };
            assert_eq!(install(home.path(), &user).unwrap(), BTreeMap::new());
        }
        assert!(!hide_kit::layout::shell_integration(home.path()).exists());
    }

    #[test]
    fn another_builds_folder_goes_only_once_no_pane_started_with_it_for_an_hour() {
        let home = tempfile::tempdir().unwrap();
        let root = hide_kit::layout::shell_integration(home.path());
        let aged = |name: &str, age: Duration| {
            let path = root.join(name);
            std::fs::create_dir_all(path.join("zsh")).unwrap();
            std::fs::File::open(&path)
                .unwrap()
                .set_modified(SystemTime::now() - age)
                .unwrap();
            path
        };
        let recent = aged("0123456789abcdef", UNUSED_FOR - Duration::from_secs(60));
        let unused = aged("fedcba9876543210", UNUSED_FOR + Duration::from_secs(60));
        let left = aged(".staging-left", UNUSED_FOR + Duration::from_secs(60));
        let env = install(home.path(), &zsh_user()).unwrap();
        let ours = Path::new(&env["ZDOTDIR"]).parent().unwrap().to_owned();
        assert!(recent.is_dir(), "a pane may still be reading it");
        assert!(!unused.exists() && !left.exists());
        // This build's folder counts as used each time a pane is started
        // with it, so another build's sweep keeps it.
        std::fs::File::open(&ours)
            .unwrap()
            .set_modified(SystemTime::now() - UNUSED_FOR * 2)
            .unwrap();
        install(home.path(), &zsh_user()).unwrap();
        let age = SystemTime::now()
            .duration_since(std::fs::metadata(&ours).unwrap().modified().unwrap())
            .unwrap();
        assert!(age < UNUSED_FOR, "{age:?}");
    }
}
