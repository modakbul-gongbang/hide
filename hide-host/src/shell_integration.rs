//! Hide's shell integration: the startup files that let a pane's shell say
//! when it has finished starting, after the user's own startup files and
//! anything they asked at the keyboard (a `[Y/n]` update question), so Hide
//! types an agent's command only then.
//!
//! It is VS Code's terminal shell integration, reached through the
//! environment alone, because Herdr starts a pane's shell itself and takes
//! only an `env` from Hide: zsh reads its startup files from `ZDOTDIR`, and
//! fish reads the `vendor_conf.d` of each folder in `XDG_DATA_DIRS`. Each file
//! here reads the user's own file of the same name first and gives the
//! environment back once the last one has run; the first prompt reports a
//! Herdr pane token, `hide_prompt`, which is the mark Hide reads (Herdr does
//! not parse OSC 133 prompt marks). bash, sh, PowerShell and cmd take their
//! startup file only as an argument, so no environment reaches them and a
//! start there is confirmed after the fact instead (`herdr-core`'s
//! `agent_start`).
//!
//! The files live under the node's account home
//! (`hide_kit::layout::shell_integration`), one folder per version of their
//! text, written once and never rewritten. A shell reads them only while it
//! starts: the first prompt has given the environment back, so nothing in the
//! pane, or anything it starts later, refers to them again. Writing a version
//! removes every other one, so the folder holds one version at a time.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

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

/// Writes the integration under `home` once and answers the environment a
/// pane's shell is started with. `data_dirs` is the `XDG_DATA_DIRS` the
/// node's own process has, the closest it knows to the one Herdr gives a
/// pane. Empty where no shell can take it (Windows).
#[cfg(unix)]
pub fn install(home: &Path, data_dirs: Option<&str>) -> io::Result<BTreeMap<String, String>> {
    let folder = version_folder(home)?;
    let data = folder.join("fish-data");
    let with = |path: PathBuf| path.to_string_lossy().into_owned();
    let inherited = data_dirs
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or(XDG_DATA_DIRS_DEFAULT);
    Ok(BTreeMap::from([
        ("ZDOTDIR".to_owned(), with(folder.join("zsh"))),
        (
            "XDG_DATA_DIRS".to_owned(),
            format!("{}:{inherited}", with(data)),
        ),
    ]))
}

#[cfg(windows)]
pub fn install(_home: &Path, _data_dirs: Option<&str>) -> io::Result<BTreeMap<String, String>> {
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
        std::fs::create_dir_all(fish.parent().ok_or_else(|| io::Error::other("no fish folder"))?)?;
        std::fs::write(&fish, FISH)
    })();
    // A writer that finished first removed this staging folder with the
    // others; its folder is the same text.
    if let Err(error) = written {
        return if folder.is_dir() { Ok(folder) } else { Err(error) };
    }
    match hide_platform::fs::atomic::rename_no_replace_path(staging.path(), &folder) {
        Ok(()) => {
            // The folder now lives at its name; nothing is left to remove.
            let _ = staging.keep();
            remove_others(&root, &folder);
            Ok(folder)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && folder.is_dir() => Ok(folder),
        Err(error) => Err(error),
    }
}

/// Removes every version but `kept`, and what an interrupted write left. A
/// shell of another build that is starting at this moment loses the files it
/// has not read yet; one build runs per state folder, so that shell's build
/// has already gone.
#[cfg(unix)]
fn remove_others(root: &Path, kept: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != kept {
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_environment_names_files_that_read_the_users_own_first() {
        let home = tempfile::tempdir().unwrap();
        let env = install(home.path(), None).unwrap();
        let zsh = Path::new(&env["ZDOTDIR"]);
        for (name, _) in ZSH {
            assert!(zsh.join(name).is_file(), "{name}");
        }
        let (data, rest) = env["XDG_DATA_DIRS"].split_once(':').unwrap();
        assert_eq!(rest, XDG_DATA_DIRS_DEFAULT);
        assert!(Path::new(data).join(FISH_FILE).is_file());
        assert!(zsh.starts_with(hide_kit::layout::shell_integration(home.path())));
        // The node's own data folders follow Hide's, and a second install
        // answers the same folder without writing it again.
        let again = install(home.path(), Some("/opt/share")).unwrap();
        assert_eq!(again["ZDOTDIR"], env["ZDOTDIR"]);
        assert_eq!(again["XDG_DATA_DIRS"], format!("{data}:/opt/share"));
        let entries = std::fs::read_dir(hide_kit::layout::shell_integration(home.path()))
            .unwrap()
            .count();
        assert_eq!(entries, 1, "no staging folder is left behind");
    }

    #[test]
    fn writing_a_version_removes_every_other_one() {
        let home = tempfile::tempdir().unwrap();
        let root = hide_kit::layout::shell_integration(home.path());
        std::fs::create_dir_all(root.join("0123456789abcdef/zsh")).unwrap();
        std::fs::create_dir_all(root.join(".staging-left")).unwrap();
        let env = install(home.path(), None).unwrap();
        let names: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(names, [Path::new(&env["ZDOTDIR"]).parent().unwrap()]);
    }
}
