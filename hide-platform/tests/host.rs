//! The contract of `hide_platform::host`, stated as what a caller observes.
//! The same file runs on macOS, Linux and Windows. The socket Herdr listens
//! on by default is checked against the pinned Herdr itself, in
//! `hide-herdr-client/tests/real_herdr.rs`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

use hide_platform::host;

#[test]
fn the_home_folder_is_an_absolute_folder_that_exists() {
    let home = host::home_dir().unwrap();
    assert!(home.is_absolute(), "{}", home.display());
    assert!(home.is_dir(), "{} is not a folder", home.display());
}

#[test]
fn the_state_folder_under_a_home_is_the_systems_own_convention() {
    let home = std::env::temp_dir().join("example-home");
    let expected = if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else if cfg!(windows) {
        home.join("AppData").join("Local")
    } else {
        home.join(".local").join("state")
    };
    assert_eq!(host::state_dir_under(&home), expected);
    // Without a variable that moves it, the account's state folder is the
    // one under its home.
    let home_only = variables(vec![(host::HOME_VARIABLE, home.clone())]);
    if !cfg!(windows) {
        assert_eq!(host::state_dir_from(&home_only).unwrap(), expected);
    }
}

#[test]
fn the_state_folder_is_the_accounts_own() {
    let state = host::state_dir().unwrap();
    assert!(state.is_absolute(), "{}", state.display());
    if std::env::var_os("XDG_STATE_HOME").is_none() {
        let home = host::home_dir().unwrap();
        assert!(
            state.starts_with(&home),
            "{} is not under {}",
            state.display(),
            home.display()
        );
    }
}

/// A folder holding a file named like the program that the system would not
/// run: no execute bit on Unix, no extension on Windows.
fn decoy(name: &str) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    fs::write(folder.path().join(name), b"not a program").unwrap();
    folder
}

fn path_with_first(first: &Path) -> std::ffi::OsString {
    let mut folders = vec![first.to_path_buf()];
    folders.extend(std::env::split_paths(&host::login_path().unwrap()));
    std::env::join_paths(folders).unwrap()
}

#[test]
fn git_is_found_on_the_login_path_and_runs() {
    let git = host::find_program(&host::login_path().unwrap(), "git").expect("git on PATH");
    assert!(git.is_absolute(), "{}", git.display());
    let output = Command::new(&git).arg("--version").output().unwrap();
    assert!(
        output.status.success(),
        "{} --version failed",
        git.display()
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("git version"));
}

#[test]
fn a_file_the_system_would_not_run_is_passed_over() {
    let decoy = decoy("git");
    let git = host::find_program(&path_with_first(decoy.path()), "git").expect("git on PATH");
    assert!(
        !git.starts_with(decoy.path()),
        "{} is not a program",
        git.display()
    );
}

#[test]
fn a_missing_program_is_not_found() {
    assert_eq!(
        host::find_program(
            &host::login_path().unwrap(),
            "hide-platform-no-such-program"
        ),
        None
    );
}

#[test]
fn the_default_shell_when_there_is_one_is_a_file_that_exists() {
    match host::default_shell() {
        Ok(shell) => {
            assert!(shell.is_absolute(), "{}", shell.display());
            assert!(shell.is_file(), "{} does not exist", shell.display());
        }
        // Windows always names its command interpreter; a Unix process may
        // be started without SHELL, and then there is no answer to give.
        Err(error) if cfg!(windows) => panic!("{error}"),
        Err(error) => {
            assert_eq!(error.kind(), ErrorKind::NotFound, "{error}");
            assert!(std::env::var_os("SHELL").is_none());
        }
    }
}

#[test]
fn the_tailscale_location_is_absolute_where_the_system_has_one() {
    match host::tailscale_cli() {
        Ok(cli) => assert!(cli.is_absolute(), "{}", cli.display()),
        Err(error) if cfg!(target_os = "linux") => {
            assert_eq!(error.kind(), ErrorKind::Unsupported, "{error}")
        }
        Err(error) => panic!("{error}"),
    }
}

#[test]
fn the_machine_names_itself_the_same_way_twice() {
    let name = host::name().unwrap();
    assert!(!name.is_empty());
    assert_eq!(host::name().unwrap(), name);
    let id = host::machine_id().unwrap();
    assert!(!id.is_empty());
    assert!(!id.chars().any(char::is_whitespace), "{id:?}");
    assert_eq!(id, id.to_lowercase(), "lineage identities use one spelling");
    assert_eq!(host::machine_id().unwrap(), id);
}

/// Where the system keeps an item it trashed, found and removed again so the
/// test leaves the account's Trash as it found it.
#[cfg(not(target_os = "macos"))]
fn take_back_from_the_trash(name: &str) -> bool {
    let items: Vec<_> = trash::os_limited::list()
        .unwrap()
        .into_iter()
        .filter(|item| item.name == std::ffi::OsStr::new(name))
        .collect();
    let found = !items.is_empty();
    trash::os_limited::purge_all(items).unwrap();
    found
}

#[test]
fn a_trashed_file_leaves_its_folder_for_the_trash() {
    let folder = tempfile::Builder::new()
        .prefix("hide-platform-trash-")
        .tempdir()
        .unwrap();
    // No extension: the Recycle Bin lists an item by its shell display name,
    // which drops an extension Explorer knows (`.txt`) under its default view.
    let name = format!(
        "{}-{}",
        folder.path().file_name().unwrap().to_str().unwrap(),
        std::process::id()
    );
    let file: PathBuf = folder.path().join(&name);
    let contents = format!("to be trashed: {name}");
    fs::write(&file, &contents).unwrap();
    // NSFileManager uses the OS account's Trash, even under an isolated
    // HOME. Register cleanup before the move so a failed assertion cannot
    // leave our item behind. Product home resolution stays HOME-aware.
    #[cfg(target_os = "macos")]
    let cleanup = macos_trash::OwnedTrash::new(
        &file,
        macos_trash::os_account_trash_dir().unwrap(),
        contents.as_bytes(),
    )
    .unwrap();
    host::trash(&file).unwrap();
    assert!(file.symlink_metadata().is_err(), "the file is still there");
    #[cfg(target_os = "macos")]
    let found = cleanup.take_back().unwrap();
    #[cfg(not(target_os = "macos"))]
    let found = take_back_from_the_trash(&name);
    assert!(found, "{name} is not in the system's Trash");
}

#[cfg(target_os = "macos")]
mod macos_trash {
    use std::ffi::{CStr, OsStr};
    use std::fs::{self, File, OpenOptions};
    use std::io::{self, Read};
    use std::mem::MaybeUninit;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::{Path, PathBuf};

    use hide_platform::fs::identity;

    pub(super) fn os_account_trash_dir() -> io::Result<PathBuf> {
        // One bounded account lookup, independent of the fixture's HOME.
        // An unusually large directory-service entry fails before trashing.
        const MAX_ACCOUNT_BYTES: usize = 64 * 1024;
        let mut buffer = vec![0u8; MAX_ACCOUNT_BYTES];
        let mut account = MaybeUninit::<libc::passwd>::zeroed();
        let mut found = std::ptr::null_mut();
        // SAFETY: both output buffers are writable and outlive the call.
        // getuid takes no arguments and reads only the calling process's uid.
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                account.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut found,
            )
        };
        if status != 0 {
            return Err(io::Error::from_raw_os_error(status));
        }
        if found.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the OS account has no user-database entry",
            ));
        }
        // SAFETY: a successful lookup with a non-null result filled account.
        let account = unsafe { account.assume_init() };
        if account.pw_dir.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the OS account has no home folder",
            ));
        }
        // SAFETY: getpwuid_r placed this NUL-terminated string in buffer,
        // which remains alive until the path has been copied.
        let bytes = unsafe { CStr::from_ptr(account.pw_dir) }.to_bytes();
        let home = PathBuf::from(OsStr::from_bytes(bytes));
        if !home.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the OS account's home is not an absolute path",
            ));
        }
        Ok(home.join(".Trash"))
    }

    pub(super) struct OwnedTrash {
        kept: PathBuf,
        // Keep the original open so its file identity cannot be reused while
        // this guard is responsible for the item, including on unwinding.
        original: File,
        contents: Vec<u8>,
    }

    impl OwnedTrash {
        pub(super) fn new(file: &Path, trash: PathBuf, contents: &[u8]) -> io::Result<Self> {
            let name = file.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "the fixture has no file name")
            })?;
            let kept = trash.join(name);
            match kept.symlink_metadata() {
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "the fixture's exact Trash name is already occupied",
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            let original = File::open(file)?;
            // This contract observes a rename, whose identity survives. Do
            // not move a fixture across volumes and then lose its identity.
            // The account home exists even before its first Trash item.
            let parent = trash.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "the Trash has no parent")
            })?;
            if identity::file_id_of(&original)?.volume() != identity::file_id(parent)?.volume() {
                return Err(io::Error::other(
                    "place the fixture temp folder on the OS account's volume before trashing",
                ));
            }
            Ok(Self {
                kept,
                original,
                contents: contents.to_vec(),
            })
        }

        pub(super) fn take_back(&self) -> io::Result<bool> {
            let metadata = match self.kept.symlink_metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            };
            let refused = || {
                io::Error::other(format!(
                    "retained {}: ownership mismatch; inspect the fixture identity before recovery",
                    self.kept.display()
                ))
            };
            if !metadata.is_file() || metadata.len() != self.contents.len() as u64 {
                return Err(refused());
            }
            let kept = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(&self.kept)?;
            let expected = identity::file_id_of(&self.original)?;
            if identity::file_id_of(&kept)? != expected {
                return Err(refused());
            }
            let mut contents = Vec::new();
            kept.take(self.contents.len() as u64 + 1)
                .read_to_end(&mut contents)?;
            if contents != self.contents || identity::file_id_nofollow(&self.kept)? != expected {
                return Err(refused());
            }
            fs::remove_file(&self.kept)?;
            Ok(true)
        }
    }

    impl Drop for OwnedTrash {
        fn drop(&mut self) {
            if let Err(error) = self.take_back() {
                let recovery = format!(
                    "fixture_trash.cleanup_failed path={} error={error}; inspect the retained item before recovery",
                    self.kept.display()
                );
                if std::thread::panicking() {
                    // The test is already failing; report recovery without a
                    // second panic aborting the remaining fixture destructors.
                    eprintln!("{recovery}");
                } else {
                    panic!("{recovery}");
                }
            }
        }
    }

    #[test]
    fn owned_trash_is_cleaned_when_an_assertion_unwinds() {
        let folder = tempfile::tempdir().unwrap();
        let trash = folder.path().join("trash");
        fs::create_dir(&trash).unwrap();
        let file = folder.path().join("owned");
        let kept = trash.join("owned");
        let unrelated = trash.join("unrelated");
        fs::write(&file, b"owned content").unwrap();
        fs::write(&unrelated, b"leave alone").unwrap();
        let failure = std::panic::catch_unwind(|| {
            let _cleanup = OwnedTrash::new(&file, trash, b"owned content").unwrap();
            fs::rename(&file, &kept).unwrap();
            panic!("a failure after the move");
        });
        assert!(failure.is_err());
        assert!(!kept.exists(), "our trashed item survived the assertion");
        assert_eq!(fs::read(unrelated).unwrap(), b"leave alone");
    }

    #[test]
    fn owned_trash_refuses_a_different_file_with_the_same_name_and_content() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("owned");
        let trash = folder.path().join("trash");
        fs::create_dir(&trash).unwrap();
        fs::write(&file, b"owned content").unwrap();
        let cleanup = OwnedTrash::new(&file, trash.clone(), b"owned content").unwrap();
        let replacement = trash.join("owned");
        fs::write(&replacement, b"owned content").unwrap();
        assert!(cleanup.take_back().is_err());
        assert_eq!(fs::read(&replacement).unwrap(), b"owned content");
        // This replacement is also ours, inside the private test folder.
        fs::remove_file(replacement).unwrap();
    }

    #[test]
    fn owned_trash_refuses_changed_content_on_the_original_file() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("owned");
        let trash = folder.path().join("trash");
        fs::create_dir(&trash).unwrap();
        fs::write(&file, b"owned content").unwrap();
        let cleanup = OwnedTrash::new(&file, trash.clone(), b"owned content").unwrap();
        let kept = trash.join("owned");
        fs::rename(&file, &kept).unwrap();
        fs::write(&kept, b"other content").unwrap();
        assert!(cleanup.take_back().is_err());
        assert_eq!(fs::read(&kept).unwrap(), b"other content");
        fs::write(&kept, b"owned content").unwrap();
        assert!(cleanup.take_back().unwrap());
    }
}

#[test]
fn trashing_a_missing_file_fails() {
    let folder = tempfile::tempdir().unwrap();
    assert!(host::trash(&folder.path().join("missing")).is_err());
}

/// A record of the environment holding only `pairs`.
fn variables(pairs: Vec<(&'static str, PathBuf)>) -> impl Fn(&str) -> Option<std::ffi::OsString> {
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.clone().into_os_string())
    }
}

#[test]
fn herdrs_default_socket_is_resolved_in_herdrs_own_order() {
    let base = std::env::temp_dir();
    let home = base.join("example-home");
    let roaming = base.join("example-roaming");
    let config = base.join("example-config");
    let socket = |folder: PathBuf| folder.join("herdr").join("herdr.sock");

    let home_only = variables(vec![(host::HOME_VARIABLE, home.clone())]);
    let expected = if cfg!(windows) {
        socket(home.join("AppData").join("Roaming"))
    } else {
        socket(home.join(".config"))
    };
    assert_eq!(
        host::herdr_socket_default_from(&home_only).unwrap(),
        expected
    );

    let with_appdata = variables(vec![
        (host::HOME_VARIABLE, home.clone()),
        ("APPDATA", roaming.clone()),
    ]);
    let expected = if cfg!(windows) {
        socket(roaming.clone())
    } else {
        socket(home.join(".config"))
    };
    assert_eq!(
        host::herdr_socket_default_from(&with_appdata).unwrap(),
        expected
    );

    let with_config = variables(vec![
        (host::HOME_VARIABLE, home.clone()),
        ("APPDATA", roaming),
        ("XDG_CONFIG_HOME", config.clone()),
    ]);
    assert_eq!(
        host::herdr_socket_default_from(&with_config).unwrap(),
        socket(config)
    );

    let relative = variables(vec![
        (host::HOME_VARIABLE, home),
        ("XDG_CONFIG_HOME", PathBuf::from("relative")),
    ]);
    assert_eq!(
        host::herdr_socket_default_from(&relative)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        host::herdr_socket_default_from(&variables(Vec::new()))
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
}
