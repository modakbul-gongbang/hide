//! What this machine and the account running on it are called, and where
//! their things live: the home folder, the folder an application keeps its
//! state in, the socket the pinned Herdr listens on by default, the programs
//! on the search path, the default shell, the Trash, the Tailscale CLI, and
//! the machine's name and identity.
//!
//! Every function answers for the machine this process runs on, from the
//! variables and tables that system defines, and refuses (`NotFound`, or
//! `Unsupported` where the system has no such thing) rather than guessing
//! when they are absent. A device helper running on another machine asks its
//! own copy, so nothing here assumes the machine is a Unix one.
//!
//! The variables read, and what an absent one means:
//!
//! | Variable | System | Absent |
//! | --- | --- | --- |
//! | `HOME` | macOS, Linux | no home folder, so no state, socket or Trash location |
//! | `USERPROFILE` | Windows | no home folder |
//! | `XDG_STATE_HOME` | Linux | state lives under `~/.local/state` |
//! | `LOCALAPPDATA` | Windows | no state folder |
//! | `XDG_CONFIG_HOME` | all, as Herdr reads it | Herdr's config folder is the system's own |
//! | `APPDATA` | Windows | Herdr's config folder is under `USERPROFILE` |
//! | `PATH` | all | no program is found |
//! | `PATHEXT` | Windows | the extensions Windows has always run |
//! | `SHELL` | macOS, Linux | no default shell |
//! | `ComSpec` | Windows | no default shell |
//! | `ProgramFiles` | Windows | no Tailscale location |

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

/// The variable that names the account's home folder.
pub const HOME_VARIABLE: &str = if cfg!(windows) { "USERPROFILE" } else { "HOME" };

/// Every variable this module reads, so a caller that keeps a registry of the
/// variables it depends on can check it holds these.
pub const VARIABLES: &[&str] = &[
    HOME_VARIABLE,
    "XDG_STATE_HOME",
    "LOCALAPPDATA",
    "XDG_CONFIG_HOME",
    "APPDATA",
    "PATH",
    "PATHEXT",
    "SHELL",
    "ComSpec",
    "ProgramFiles",
];

/// How a caller with its own record of the environment (a registry that
/// validates at start, a test) hands it to the `_from` functions: the value
/// of a variable, `None` when it is unset.
pub type Variables<'a> = &'a dyn Fn(&str) -> Option<OsString>;

fn process(name: &str) -> Option<OsString> {
    std::env::var_os(name)
}

/// The account's home folder.
pub fn home_dir() -> io::Result<PathBuf> {
    home_dir_from(&process)
}

/// [`home_dir`] from the caller's variables.
pub fn home_dir_from(variables: Variables) -> io::Result<PathBuf> {
    absolute_variable(variables, HOME_VARIABLE)
}

/// The folder applications keep the account's state in on this system:
/// `~/Library/Application Support` on macOS, `$XDG_STATE_HOME` or
/// `~/.local/state` on Linux, `%LOCALAPPDATA%` on Windows. A caller keeps its
/// own folder inside it.
pub fn state_dir() -> io::Result<PathBuf> {
    state_dir_from(&process)
}

/// [`state_dir`] from the caller's variables.
pub fn state_dir_from(variables: Variables) -> io::Result<PathBuf> {
    if cfg!(windows) {
        absolute_variable(variables, "LOCALAPPDATA")
    } else if !cfg!(target_os = "macos") && nonempty_variable(variables, "XDG_STATE_HOME").is_some()
    {
        absolute_variable(variables, "XDG_STATE_HOME")
    } else {
        Ok(state_dir_under(&home_dir_from(variables)?))
    }
}

/// The state folder this system's convention puts under `home` when no
/// variable moves it: `Library/Application Support` on macOS,
/// `.local/state` on Linux, `AppData\Local` on Windows. It is for a caller
/// whose state follows a home it was handed (an isolated run) rather than
/// the account's.
pub fn state_dir_under(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library").join("Application Support")
    } else if cfg!(windows) {
        home.join("AppData").join("Local")
    } else {
        home.join(".local").join("state")
    }
}

/// The socket the pinned Herdr's default session listens on when nothing
/// overrides it: `herdr.sock` in Herdr's config folder, which is
/// `$XDG_CONFIG_HOME/herdr` wherever that is set, and otherwise
/// `~/.config/herdr` on macOS and Linux and `%APPDATA%\herdr` on Windows
/// (whose socket is a named pipe of that name). `HERDR_SOCKET_PATH` and a
/// named session are the caller's to apply before asking.
pub fn herdr_socket_default() -> io::Result<PathBuf> {
    herdr_socket_default_from(&process)
}

/// [`herdr_socket_default`] from the caller's variables.
pub fn herdr_socket_default_from(variables: Variables) -> io::Result<PathBuf> {
    Ok(herdr_config_dir(variables)?.join("herdr.sock"))
}

/// Herdr's config folder, resolved in Herdr's own order (`config::io` of the
/// pinned release); where Herdr would fall back to a temporary folder, this
/// says there is none.
fn herdr_config_dir(variables: Variables) -> io::Result<PathBuf> {
    if variables("XDG_CONFIG_HOME").is_some() {
        return Ok(absolute_variable(variables, "XDG_CONFIG_HOME")?.join("herdr"));
    }
    if cfg!(windows) {
        if variables("APPDATA").is_some() {
            return Ok(absolute_variable(variables, "APPDATA")?.join("herdr"));
        }
        return Ok(home_dir_from(variables)?
            .join("AppData")
            .join("Roaming")
            .join("herdr"));
    }
    Ok(home_dir_from(variables)?.join(".config").join("herdr"))
}

/// The search path this process finds programs on (`PATH`). The desktop app
/// adds the folders installers put programs in before it starts `hide`, so
/// this is the login shell's path even when the app was opened from the Dock.
pub fn login_path() -> io::Result<OsString> {
    std::env::var_os("PATH")
        .filter(|path| !path.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "PATH is not set"))
}

/// The program `name` as this system's shell would find it on `path`: the
/// first folder holding it, where a file without an execute bit is not a
/// program on Unix, and on Windows a name without an extension is tried with
/// each extension `PATHEXT` names.
pub fn find_program(path: &OsStr, name: &str) -> Option<PathBuf> {
    let names = program_names(name);
    std::env::split_paths(path)
        .filter(|folder| folder.is_absolute())
        .flat_map(|folder| names.iter().map(move |name| folder.join(name)))
        .find(|candidate| is_program(candidate))
}

fn program_names(name: &str) -> Vec<OsString> {
    if !cfg!(windows) || Path::new(name).extension().is_some() {
        return vec![OsString::from(name)];
    }
    let extensions = std::env::var("PATHEXT")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
    extensions
        .split(';')
        .filter(|extension| extension.starts_with('.'))
        .map(|extension| OsString::from(format!("{name}{}", extension.to_ascii_lowercase())))
        .collect()
}

fn is_program(candidate: &Path) -> bool {
    candidate.is_file() && crate::fs::permissions::is_executable(candidate).unwrap_or(false)
}

/// The shell the account runs commands in by default: `$SHELL` on macOS and
/// Linux, `%ComSpec%` (cmd.exe) on Windows.
pub fn default_shell() -> io::Result<PathBuf> {
    absolute_variable(&process, if cfg!(windows) { "ComSpec" } else { "SHELL" })
}

/// Moves the file or folder at `path` to the system's Trash (the Recycle
/// Bin on Windows, the account's freedesktop trash on Linux), from which the
/// operator can restore it. On macOS the item goes through `NSFileManager`,
/// not Finder: Finder's route runs `osascript` and asks for Automation
/// permission on first use, which an SSH session cannot answer, and the file
/// manager's needs neither a permission nor a subprocess. What it gives up is
/// Finder's "Put Back" on some systems.
pub fn trash(path: &Path) -> io::Result<()> {
    // Only the macOS branch below changes the context.
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context.delete(path).map_err(|error| match error {
        trash::Error::CouldNotAccess { .. } => {
            io::Error::new(io::ErrorKind::PermissionDenied, "it is not accessible")
        }
        trash::Error::TargetedRoot => {
            io::Error::new(io::ErrorKind::InvalidInput, "it is a volume root")
        }
        trash::Error::Unknown { description } | trash::Error::Os { description, .. } => {
            io::Error::other(description)
        }
        other => io::Error::other(other.to_string()),
    })
}

/// Where the Tailscale installer puts its CLI outside the search path: the
/// app bundle's binary on macOS, `%ProgramFiles%\Tailscale\tailscale.exe` on
/// Windows. Linux packages put `tailscale` on the search path, so there is no
/// such place there (`Unsupported`), and [`find_program`] finds it.
pub fn tailscale_cli() -> io::Result<PathBuf> {
    if cfg!(target_os = "macos") {
        Ok(PathBuf::from(
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        ))
    } else if cfg!(windows) {
        Ok(absolute_variable(&process, "ProgramFiles")?
            .join("Tailscale")
            .join("tailscale.exe"))
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Linux installs tailscale on the search path",
        ))
    }
}

/// The machine's host name, as the system reports it.
pub fn name() -> io::Result<String> {
    let name = sys::name()?;
    let name = name.trim();
    if name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the system reports an empty host name",
        ));
    }
    Ok(name.to_owned())
}

/// The machine's own identity, which survives a rename and a reinstall of
/// Hide: the hardware UUID on macOS (`IOPlatformUUID`), `/etc/machine-id` on
/// Linux, the `MachineGuid` Windows keeps in its registry.
pub fn machine_id() -> io::Result<String> {
    let id = sys::machine_id()?;
    let id = id.trim();
    if id.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the system reports an empty machine id",
        ));
    }
    Ok(id.to_owned())
}

fn nonempty_variable(variables: Variables, name: &str) -> Option<OsString> {
    variables(name).filter(|value| !value.is_empty())
}

/// The variable `name` as an absolute path: `NotFound` when it is unset or
/// empty, `InvalidInput` when it names a relative path.
fn absolute_variable(variables: Variables, name: &str) -> io::Result<PathBuf> {
    let value = nonempty_variable(variables, name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{name} is not set")))?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name} is not an absolute path"),
        ));
    }
    Ok(path)
}

#[cfg(unix)]
mod sys {
    use std::io;

    pub(super) fn name() -> io::Result<String> {
        let mut buffer = [0u8; 256];
        // SAFETY: the buffer outlives the call and its length is passed with it.
        if unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let end = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        Ok(String::from_utf8_lossy(&buffer[..end]).into_owned())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn machine_id() -> io::Result<String> {
        let output = std::process::Command::new("/usr/sbin/ioreg")
            .args(["-rd1", "-c", "IOPlatformExpertDevice"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "ioreg exited with {}",
                output.status
            )));
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| {
                let (_, value) = line.split_once("IOPlatformUUID")?;
                value.split('"').nth(1).map(str::to_owned)
            })
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ioreg names no IOPlatformUUID"))
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn machine_id() -> io::Result<String> {
        std::fs::read_to_string("/etc/machine-id")
    }
}

#[cfg(windows)]
mod sys {
    use std::ffi::OsString;
    use std::io;
    use std::os::windows::ffi::OsStringExt;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
    };
    use windows_sys::Win32::System::SystemInformation::{
        ComputerNameDnsHostname, GetComputerNameExW,
    };

    pub(super) fn name() -> io::Result<String> {
        let mut buffer = vec![0u16; 256];
        loop {
            let mut length = buffer.len() as u32;
            // SAFETY: `buffer` holds `length` writable units.
            if unsafe {
                GetComputerNameExW(ComputerNameDnsHostname, buffer.as_mut_ptr(), &mut length)
            } != 0
            {
                return Ok(OsString::from_wide(&buffer[..length as usize])
                    .to_string_lossy()
                    .into_owned());
            }
            let error = io::Error::last_os_error();
            // ERROR_MORE_DATA: `length` now holds the size the name needs.
            if error.raw_os_error() != Some(234) || length as usize <= buffer.len() {
                return Err(error);
            }
            buffer.resize(length as usize, 0);
        }
    }

    pub(super) fn machine_id() -> io::Result<String> {
        let key = widestring::U16CString::from_str("SOFTWARE\\Microsoft\\Cryptography")
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let value = widestring::U16CString::from_str("MachineGuid")
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let mut buffer = vec![0u16; 64];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: the names are NUL-terminated and `buffer` holds `size`
        // writable bytes. The 64-bit view is asked for, so a 32-bit process
        // reads the same id as everything else on the machine.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
                null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(status as i32));
        }
        let units = (size as usize / 2).saturating_sub(1);
        Ok(OsString::from_wide(&buffer[..units])
            .to_string_lossy()
            .into_owned())
    }
}
