//! Single registry for every environment key this crate reads.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use hide_platform::host;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvKey {
    pub key: &'static str,
    pub required: bool,
    pub format: &'static str,
    pub absent_behavior: &'static str,
}

pub const HERDR_SOCKET_PATH: &str = "HERDR_SOCKET_PATH";
pub const HERDR_BIN_PATH: &str = "HERDR_BIN_PATH";
pub const PATH: &str = "PATH";
pub const XDG_STATE_HOME: &str = "XDG_STATE_HOME";
pub const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";
pub const APPDATA: &str = "APPDATA";
pub const PATHEXT: &str = "PATHEXT";
pub const HIDE_STATE_DIR: &str = "HIDE_STATE_DIR";
pub const HIDE_KEEP_ALIVE: &str = "HIDE_KEEP_ALIVE";
pub const HIDE_VITE_ORIGIN: &str = "HIDE_VITE_ORIGIN";
pub const HIDE_PORT: &str = "HIDE_PORT";
pub const HIDE_IDLE_SECS: &str = "HIDE_IDLE_SECS";
/// `HOME`, or `USERPROFILE` on Windows.
pub const HOME: &str = host::HOME_VARIABLE;
pub const HIDE_OPEN_COMMAND: &str = "HIDE_OPEN_COMMAND";
pub const HIDE_HOST_HELPER_ROOT: &str = "HIDE_HOST_HELPER_ROOT";
pub const HIDE_HOST_CLI_DIR: &str = "HIDE_HOST_CLI_DIR";
pub const HERDR_PANE_ID: &str = "HERDR_PANE_ID";
pub const HIDE_CAP_REF: &str = "HIDE_CAP_REF";
pub const HIDE_TAILSCALE_BIN: &str = "HIDE_TAILSCALE_BIN";

pub const REGISTRY: &[EnvKey] = &[
    EnvKey {
        key: HERDR_SOCKET_PATH,
        required: false,
        format: "absolute local socket path (a named pipe's name on Windows)",
        absent_behavior: "The socket Herdr's default session uses (see XDG_CONFIG_HOME) when it exists; otherwise core starts without a Herdr socket and the sidebar shows that state",
    },
    EnvKey {
        key: HERDR_BIN_PATH,
        required: false,
        format: "absolute path of an executable herdr binary; Herdr sets it in every pane it manages, and a daemon refuses to start on a path that no longer exists",
        absent_behavior: "The first `herdr` on PATH attaches pane terminals; with neither, no pane terminal can attach and the daemon logs it",
    },
    EnvKey {
        key: PATH,
        required: false,
        format: "the system's executable search path (colon-separated, semicolons on Windows)",
        absent_behavior: "Only HERDR_BIN_PATH can name the herdr binary",
    },
    EnvKey {
        key: PATHEXT,
        required: false,
        format: "Windows only: semicolon-separated extensions a program on PATH may have",
        absent_behavior: "A program on PATH is found as .COM, .EXE, .BAT or .CMD",
    },
    EnvKey {
        key: XDG_STATE_HOME,
        required: false,
        format: "absolute directory path",
        absent_behavior: "State lives under $HOME/.hide/state; a set value keeps $XDG_STATE_HOME/hide and moves nothing",
    },
    EnvKey {
        key: XDG_CONFIG_HOME,
        required: false,
        format: "absolute directory path",
        absent_behavior: "Herdr's default socket is under ~/.config/herdr (%APPDATA%\\herdr on Windows), as Herdr itself resolves it",
    },
    EnvKey {
        key: APPDATA,
        required: false,
        format: "Windows only: absolute directory path",
        absent_behavior: "Herdr's default socket on Windows is under %USERPROFILE%\\AppData\\Roaming\\herdr",
    },
    EnvKey {
        key: HIDE_STATE_DIR,
        required: false,
        format: "absolute directory path",
        absent_behavior: "State lives under $XDG_STATE_HOME/hide when that is set, otherwise $HOME/.hide/state, where `hide connect` moves a legacy $HOME/.local/state/hide once",
    },
    EnvKey {
        key: HIDE_KEEP_ALIVE,
        required: false,
        format: "1 or true",
        absent_behavior: "Daemon exits 10 minutes after the last WebSocket client disconnects",
    },
    EnvKey {
        key: HIDE_VITE_ORIGIN,
        required: false,
        format: "http://127.0.0.1:<port>",
        absent_behavior: "Only the daemon origin is allowed on the WebSocket",
    },
    EnvKey {
        key: HIDE_PORT,
        required: false,
        format: "TCP port 1-65535",
        absent_behavior: "Bind 127.0.0.1:0 and write the chosen port to the state file",
    },
    EnvKey {
        key: HIDE_IDLE_SECS,
        required: false,
        format: "positive integer seconds",
        absent_behavior: "Idle timeout is 600 seconds",
    },
    EnvKey {
        key: HIDE_OPEN_COMMAND,
        required: false,
        format: "absolute path of an executable CLI helper whose first argument is the file to open",
        absent_behavior: "The host OS handler opens it (macOS `open`, Windows ShellExecuteW association, Linux `xdg-open`)",
    },
    EnvKey {
        key: HIDE_HOST_HELPER_ROOT,
        required: false,
        format: "absolute path, or a path under the device's home spelled `~/...`, with no `.` or `..` segment",
        absent_behavior: "The device helper installs under ~/.hide/host-helper on each consented device (a consent for the legacy ~/.local/share/hide/host-helper is carried there without asking); a different value is a different consent scope, so every device asks again (isolated verification sets it to a temporary folder)",
    },
    EnvKey {
        key: HIDE_HOST_CLI_DIR,
        required: false,
        format: "absolute path, or a path under the device's home spelled `~/...`, with no `.` or `..` segment",
        absent_behavior: "Each consented device links `hide` in ~/.local/bin to the command installed beside its helper, when that name is free or already Hide's link; a different value is a different consent scope, so every device asks again (isolated verification sets it to a temporary folder so no test writes the account's own ~/.local/bin)",
    },
    EnvKey {
        key: HERDR_PANE_ID,
        required: false,
        format: "the Herdr pane id; Herdr sets it in every pane it manages",
        absent_behavior: "Pane-scoped Workspace commands refuse because the caller cannot identify a connected Herdr pane",
    },
    EnvKey {
        key: HIDE_CAP_REF,
        required: false,
        format: "absolute path to a regular, owner-only pane credential reference",
        absent_behavior: "Direct Herdr pane callers try kernel peer bootstrap; detached agent tools require a session-scoped reference",
    },
    EnvKey {
        key: HIDE_TAILSCALE_BIN,
        required: false,
        format: "absolute path of the tailscale CLI Settings > Mobile runs; a path that does not exist reads as Tailscale not installed",
        absent_behavior: "The CLI the system's Tailscale app installs (/Applications/Tailscale.app/Contents/MacOS/Tailscale on macOS, %ProgramFiles%\\Tailscale\\tailscale.exe on Windows), then `tailscale` on PATH; isolated verification sets it so no test reaches the account's own Tailscale",
    },
    EnvKey {
        key: HOME,
        required: true,
        format: "absolute home-directory path",
        absent_behavior: "Boot fails; the state directory cannot be resolved",
    },
];

#[derive(Clone, Debug)]
pub struct Env {
    /// The filesystem boundary root for the web shell's listing and
    /// registration events (`boundary.rs`); read once, never configured.
    pub home: PathBuf,
    pub herdr_socket_path: Option<String>,
    /// The binary the core spawns for `herdr terminal session control`; the
    /// core refuses every pane attach without one.
    pub herdr_bin_path: Option<PathBuf>,
    pub state_dir: PathBuf,
    /// The folder builds before `~/.hide` kept state in, when `state_dir` is
    /// the default one: `hide connect` moves it there once (PRD
    /// hide-home-layout D-05). `None` when HIDE_STATE_DIR or XDG_STATE_HOME
    /// chose another folder, which is never moved.
    pub legacy_state_dir: Option<PathBuf>,
    pub keep_alive: bool,
    pub vite_origin: Option<String>,
    pub bind: SocketAddr,
    pub idle_secs: u64,
    /// This daemon's build (`build_id`), set by `hided serve` from its own
    /// executable; never read from the environment. A daemon started in
    /// process (tests) has none and reports none.
    pub build: Option<String>,
    /// Optional test/operator-selected host opener, validated before serving.
    pub open_command: Option<PathBuf>,
    /// Where the device helper is installed on each SSH device; part of the
    /// consent scope the operator agrees to (PRD S5.5 D-23).
    pub host_helper_root: Option<String>,
    /// Where each SSH device links its `hide` command; part of the same
    /// consent scope.
    pub host_cli_dir: Option<String>,
    /// The pane a Workspace CLI command runs in; the daemon verifies its
    /// live membership and never trusts a caller-supplied Workspace.
    pub pane_id: Option<String>,
    /// The only tailscale CLI Mobile runs, when set (`HIDE_TAILSCALE_BIN`).
    pub tailscale_bin: Option<PathBuf>,
    /// PATH as the daemon received it, searched for `tailscale` at each check.
    pub search_path: Option<String>,
}

#[derive(Debug)]
pub struct EnvError {
    pub key: &'static str,
    pub kind: &'static str,
}

impl std::fmt::Display for EnvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.kind)
    }
}

impl std::error::Error for EnvError {}

/// The keys that decide which state folder a command acts on: `HOME` (the
/// default folder is under it), `HIDE_STATE_DIR` and `XDG_STATE_HOME`.
/// `hide stop` and `hide status --json` read nothing else.
pub const STATE_FOLDER_KEYS: &[&str] = &[HOME, HIDE_STATE_DIR, XDG_STATE_HOME];

/// What `hide status` reads: the state folder, and `HIDE_IDLE_SECS`, which it
/// prints as the idle time when the daemon's `/health` gives none.
pub const STATUS_KEYS: &[&str] = &[HOME, HIDE_STATE_DIR, XDG_STATE_HOME, HIDE_IDLE_SECS];

/// Every key, checked: the daemon and every command that acts on its whole
/// configuration refuse on any invalid key, all reported at once.
pub fn load() -> Result<Env, Vec<EnvError>> {
    load_from(|key| std::env::var(key).ok())
}

/// Only `keys` are checked, for a command that reads nothing else (the sets
/// above). A key outside the set is not looked at, so the command must read
/// only the fields those keys fill; an invalid key inside the set refuses,
/// named, and is never replaced by its default.
pub fn load_for(keys: &[&str]) -> Result<Env, Vec<EnvError>> {
    load_for_from(keys, |key| std::env::var(key).ok())
}

pub fn load_for_from(
    keys: &[&str],
    read: impl FnMut(&str) -> Option<String>,
) -> Result<Env, Vec<EnvError>> {
    let (env, errors) = resolve(read);
    let errors: Vec<EnvError> = errors
        .into_iter()
        .filter(|error| keys.contains(&error.key))
        .collect();
    if errors.is_empty() {
        Ok(env)
    } else {
        Err(errors)
    }
}

pub fn load_from(read: impl FnMut(&str) -> Option<String>) -> Result<Env, Vec<EnvError>> {
    let (env, errors) = resolve(read);
    if errors.is_empty() {
        Ok(env)
    } else {
        Err(errors)
    }
}

/// Reads every key once: the environment, and what was wrong with each key
/// that was invalid (whose field then holds a placeholder no caller may use).
fn resolve(mut read: impl FnMut(&str) -> Option<String>) -> (Env, Vec<EnvError>) {
    let mut errors = Vec::new();
    let home = match read(HOME) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => {
            errors.push(EnvError {
                key: HOME,
                kind: "missing",
            });
            PathBuf::new()
        }
    };
    // What `hide_platform::host` resolves the default socket from, read
    // through this registry like every other key.
    let host_values: Vec<(&str, Option<OsString>)> = [HOME, XDG_CONFIG_HOME, APPDATA]
        .into_iter()
        .map(|key| (key, read(key).map(OsString::from)))
        .collect();
    let host_variables = |key: &str| {
        host_values
            .iter()
            .find(|(known, _)| *known == key)
            .and_then(|(_, value)| value.clone())
    };
    let herdr_socket_path = match read(HERDR_SOCKET_PATH) {
        Some(value) if value.is_empty() => {
            errors.push(EnvError {
                key: HERDR_SOCKET_PATH,
                kind: "empty",
            });
            None
        }
        // Pane credentials are proven against this socket, which has to name
        // one file wherever the daemon runs from.
        Some(value) if !Path::new(&value).is_absolute() => {
            errors.push(EnvError {
                key: HERDR_SOCKET_PATH,
                kind: "invalid",
            });
            None
        }
        Some(value) => Some(value),
        None => host::herdr_socket_default_from(&host_variables)
            .ok()
            .filter(|default| default.exists())
            .map(|default| default.display().to_string()),
    };
    let herdr_bin_path = match read(HERDR_BIN_PATH) {
        Some(value) if value.is_empty() => {
            errors.push(EnvError {
                key: HERDR_BIN_PATH,
                kind: "empty",
            });
            None
        }
        Some(value) => Some(PathBuf::from(value)),
        None => read(PATH).and_then(|path| host::find_program(path.as_ref(), "herdr")),
    };
    let hide_state_dir = read(HIDE_STATE_DIR);
    if hide_state_dir.as_deref() == Some("") {
        errors.push(EnvError {
            key: HIDE_STATE_DIR,
            kind: "empty",
        });
    }
    let state_dir = hide_kit::layout::state_dir(
        &home,
        hide_state_dir.as_deref(),
        read(XDG_STATE_HOME).as_deref(),
    );
    // Judged by the folder, not by which variable named it: `hide connect`
    // hands the daemon it starts the default folder through HIDE_STATE_DIR.
    // Only a Mac ever had the old folder.
    let legacy_state_dir = (cfg!(unix) && state_dir == hide_kit::layout::default_state_dir(&home))
        .then(|| hide_kit::layout::legacy_state_dir(&home));
    let keep_alive = match read(HIDE_KEEP_ALIVE).as_deref() {
        None => false,
        Some("1" | "true" | "TRUE") => true,
        Some(_) => {
            errors.push(EnvError {
                key: HIDE_KEEP_ALIVE,
                kind: "invalid",
            });
            false
        }
    };
    let vite_origin = match read(HIDE_VITE_ORIGIN) {
        Some(value) if value.is_empty() => {
            errors.push(EnvError {
                key: HIDE_VITE_ORIGIN,
                kind: "empty",
            });
            None
        }
        Some(value)
            if !(value.starts_with("http://127.0.0.1:")
                || value.starts_with("http://localhost:")) =>
        {
            errors.push(EnvError {
                key: HIDE_VITE_ORIGIN,
                kind: "invalid",
            });
            None
        }
        other => other,
    };
    let port = match read(HIDE_PORT) {
        None => 0_u16,
        Some(value) => match value.parse::<u16>() {
            Ok(port) => port,
            Err(_) => {
                errors.push(EnvError {
                    key: HIDE_PORT,
                    kind: "invalid",
                });
                0
            }
        },
    };
    let idle_secs = match read(HIDE_IDLE_SECS) {
        None => 600,
        Some(value) => match value.parse::<u64>() {
            Ok(secs) if secs > 0 => secs,
            _ => {
                errors.push(EnvError {
                    key: HIDE_IDLE_SECS,
                    kind: "invalid",
                });
                600
            }
        },
    };
    let open_command = match read(HIDE_OPEN_COMMAND) {
        Some(value) if !valid_program_path(Path::new(&value)) => {
            errors.push(EnvError {
                key: HIDE_OPEN_COMMAND,
                kind: "invalid",
            });
            None
        }
        Some(value) => Some(PathBuf::from(value)),
        None => None,
    };
    let host_helper_root = match read(HIDE_HOST_HELPER_ROOT) {
        Some(value) if !valid_helper_root(&value) => {
            errors.push(EnvError {
                key: HIDE_HOST_HELPER_ROOT,
                kind: "invalid",
            });
            None
        }
        other => other,
    };
    let host_cli_dir = match read(HIDE_HOST_CLI_DIR) {
        Some(value) if !valid_helper_root(&value) => {
            errors.push(EnvError {
                key: HIDE_HOST_CLI_DIR,
                kind: "invalid",
            });
            None
        }
        other => other,
    };
    let pane_id = match read(HERDR_PANE_ID) {
        Some(value) if value.is_empty() => {
            errors.push(EnvError {
                key: HERDR_PANE_ID,
                kind: "empty",
            });
            None
        }
        other => other,
    };
    let tailscale_bin = match read(HIDE_TAILSCALE_BIN) {
        Some(value)
            if !Path::new(&value).is_absolute() || value.bytes().any(|b| b.is_ascii_control()) =>
        {
            errors.push(EnvError {
                key: HIDE_TAILSCALE_BIN,
                kind: "invalid",
            });
            None
        }
        Some(value) => Some(PathBuf::from(value)),
        None => None,
    };
    let search_path = read(PATH).filter(|value| !value.is_empty());
    let env = Env {
        home,
        herdr_socket_path,
        herdr_bin_path,
        state_dir,
        legacy_state_dir,
        keep_alive,
        vite_origin,
        bind: SocketAddr::from(([127, 0, 0, 1], port)),
        idle_secs,
        build: None,
        open_command,
        host_helper_root,
        host_cli_dir,
        pane_id,
        tailscale_bin,
        search_path,
    };
    (env, errors)
}

/// A helper root names a folder on another machine, so it is checked for
/// shape only: absolute or under that machine's home, one line, and no
/// segment that could climb out of what the operator agreed to.
fn valid_helper_root(value: &str) -> bool {
    let rest = if let Some(rest) = value.strip_prefix("~/") {
        rest
    } else if let Some(rest) = value.strip_prefix('/') {
        rest
    } else {
        return false;
    };
    !rest.is_empty()
        && !value.chars().any(char::is_control)
        && rest
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn valid_program_path(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_file() {
        return false;
    }
    // What Windows starts as a program: an executable, or a batch file that
    // runs through `cmd.exe`.
    #[cfg(windows)]
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["exe", "com", "bat", "cmd"]
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path
            .metadata()
            .map_or(true, |meta| meta.permissions().mode() & 0o111 == 0)
        {
            return false;
        }
    }
    true
}

/// Why the herdr binary the environment resolved cannot be run, or `None`
/// when there is none or it runs. Herdr hands every pane the path its server
/// started from, and that path dies when the app bundle is replaced under a
/// running server, so a daemon started inside a pane would inherit a name
/// with nothing behind it and every pane attach would fail one by one. The
/// check runs where a daemon is about to rely on the binary, not on every
/// CLI call: a Workspace command never runs herdr.
pub fn herdr_bin_error(env: &Env) -> Option<String> {
    let path = env.herdr_bin_path.as_ref()?;
    (!valid_program_path(path)).then(|| {
        format!(
            "{HERDR_BIN_PATH}: {} is not an executable file; the Herdr that set it has moved, so unset it or name the herdr this app ships",
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn from_map(pairs: &[(&str, &str)]) -> Result<Env, Vec<EnvError>> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        load_from(move |key| map.get(key).cloned())
    }

    fn scoped(keys: &[&str], pairs: &[(&str, &str)]) -> Result<Env, Vec<EnvError>> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        load_for_from(keys, move |key| map.get(key).cloned())
    }

    /// An invalid value for every key outside the state folder.
    const UNRELATED_INVALID: &[(&str, &str)] = &[
        (HIDE_PORT, "abc"),
        (HIDE_OPEN_COMMAND, "relative-opener"),
        (HIDE_KEEP_ALIVE, "maybe"),
        (HIDE_VITE_ORIGIN, "http://example.com:1"),
        (HERDR_SOCKET_PATH, ""),
        (HERDR_PANE_ID, ""),
        (HIDE_HOST_HELPER_ROOT, "relative"),
        (HIDE_TAILSCALE_BIN, "relative"),
    ];

    #[test]
    fn a_relative_herdr_socket_is_refused_by_name() {
        let err =
            from_map(&[(HOME, "/Users/example"), (HERDR_SOCKET_PATH, "herdr.sock")]).unwrap_err();
        assert_eq!(
            err.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [format!("{HERDR_SOCKET_PATH}: invalid")]
        );
    }

    #[test]
    fn the_state_folder_is_all_stop_and_status_json_ask_for() {
        let mut pairs = vec![(HOME, "/Users/example"), (HIDE_IDLE_SECS, "0")];
        pairs.extend_from_slice(UNRELATED_INVALID);
        assert!(from_map(&pairs).is_err(), "the whole set is invalid");
        let env = scoped(STATE_FOLDER_KEYS, &pairs).unwrap();
        assert_eq!(env.state_dir, PathBuf::from("/Users/example/.hide/state"));
    }

    #[test]
    fn an_invalid_state_folder_key_is_refused_by_name_not_defaulted() {
        let err = scoped(
            STATE_FOLDER_KEYS,
            &[(HOME, "/Users/example"), (HIDE_STATE_DIR, "")],
        )
        .unwrap_err();
        assert_eq!(
            err.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [format!("{HIDE_STATE_DIR}: empty")]
        );
        let err = scoped(STATE_FOLDER_KEYS, UNRELATED_INVALID).unwrap_err();
        assert_eq!(
            err.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [format!("{HOME}: missing")],
            "only the key the command reads is named"
        );
    }

    #[test]
    fn status_reads_the_idle_time_as_well() {
        let mut pairs = vec![(HOME, "/Users/example")];
        pairs.extend_from_slice(UNRELATED_INVALID);
        let env = scoped(STATUS_KEYS, &pairs).unwrap();
        assert_eq!(env.idle_secs, 600);
        pairs.push((HIDE_IDLE_SECS, "0"));
        let err = scoped(STATUS_KEYS, &pairs).unwrap_err();
        assert_eq!(
            err.iter().map(ToString::to_string).collect::<Vec<_>>(),
            [format!("{HIDE_IDLE_SECS}: invalid")]
        );
    }

    #[test]
    fn a_command_set_names_only_registered_keys() {
        for key in STATE_FOLDER_KEYS.iter().chain(STATUS_KEYS) {
            assert!(
                REGISTRY.iter().any(|entry| entry.key == *key),
                "{key} is not in REGISTRY"
            );
        }
    }

    #[test]
    fn the_daemon_check_still_reports_every_invalid_key_at_once() {
        let mut pairs = vec![(HOME, "/Users/example")];
        pairs.extend_from_slice(UNRELATED_INVALID);
        let err = from_map(&pairs).unwrap_err();
        for (key, _) in UNRELATED_INVALID {
            assert!(
                err.iter().any(|error| error.key == *key),
                "{key} unreported"
            );
        }
    }

    #[test]
    fn missing_home_fails_boot() {
        let err = from_map(&[]).unwrap_err();
        assert_eq!(err[0].key, HOME);
        assert_eq!(err[0].kind, "missing");
    }

    #[test]
    fn defaults_state_dir_under_home() {
        let env = from_map(&[(HOME, "/Users/example")]).unwrap();
        assert_eq!(env.state_dir, PathBuf::from("/Users/example/.hide/state"));
        assert_eq!(
            env.legacy_state_dir.as_deref(),
            cfg!(unix).then_some(Path::new("/Users/example/.local/state/hide"))
        );
        for relocated in [
            ("HIDE_STATE_DIR", "/isolated/state"),
            ("XDG_STATE_HOME", "/xdg"),
        ] {
            let env = from_map(&[("HOME", "/Users/example"), relocated]).unwrap();
            assert_eq!(env.legacy_state_dir, None, "{relocated:?}");
        }
        assert_eq!(
            from_map(&[("HOME", "/Users/example"), ("XDG_STATE_HOME", "/xdg")])
                .unwrap()
                .state_dir,
            PathBuf::from("/xdg/hide")
        );
        assert_eq!(env.idle_secs, 600);
        assert!(!env.keep_alive);
        assert!(env.herdr_socket_path.is_none());
        assert!(env.herdr_bin_path.is_none());
    }

    #[test]
    fn herdr_bin_path_wins_over_path_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = dir
            .path()
            .join(if cfg!(windows) { "herdr.exe" } else { "herdr" });
        std::fs::write(&on_path, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&on_path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = from_map(&[
            (HOME, "/Users/example"),
            (
                "PATH",
                &std::env::join_paths([Path::new("/nonexistent"), dir.path()])
                    .unwrap()
                    .into_string()
                    .unwrap(),
            ),
        ])
        .unwrap();
        assert_eq!(env.herdr_bin_path.as_deref(), Some(on_path.as_path()));
        let env = from_map(&[
            (HOME, "/Users/example"),
            ("PATH", &dir.path().display().to_string()),
            ("HERDR_BIN_PATH", "/opt/herdr/bin/herdr"),
        ])
        .unwrap();
        assert_eq!(
            env.herdr_bin_path.as_deref(),
            Some(Path::new("/opt/herdr/bin/herdr"))
        );
        let err = from_map(&[(HOME, "/Users/example"), ("HERDR_BIN_PATH", "")]).unwrap_err();
        assert_eq!(err[0].key, HERDR_BIN_PATH);
    }

    #[test]
    fn stale_herdr_bin_path_is_named_and_a_runnable_one_passes() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("herdr-runtime/herdr");
        let env = from_map(&[
            (HOME, "/Users/example"),
            ("HERDR_BIN_PATH", &gone.display().to_string()),
        ])
        .unwrap();
        let error = herdr_bin_error(&env).expect("a path with nothing behind it is an error");
        assert!(error.starts_with("HERDR_BIN_PATH: "), "{error}");
        assert!(error.contains(&gone.display().to_string()), "{error}");

        let runnable = dir.path().join("herdr");
        std::fs::write(&runnable, b"#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&runnable, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let env = from_map(&[
            (HOME, "/Users/example"),
            ("HERDR_BIN_PATH", &runnable.display().to_string()),
        ])
        .unwrap();
        assert_eq!(herdr_bin_error(&env), None);

        let env = from_map(&[(HOME, "/Users/example"), ("PATH", "/nonexistent")]).unwrap();
        assert_eq!(
            herdr_bin_error(&env),
            None,
            "no binary at all is the logged degraded state, not a refusal"
        );
    }

    #[test]
    fn vite_origin_must_be_loopback() {
        let err = from_map(&[
            (HOME, "/Users/example"),
            ("HIDE_VITE_ORIGIN", "http://example.com"),
        ])
        .unwrap_err();
        assert_eq!(err[0].key, HIDE_VITE_ORIGIN);
    }

    #[test]
    fn helper_root_is_absolute_or_under_home_and_never_climbs() {
        for bad in [
            "relative/dir",
            "~",
            "~/",
            "/",
            "/a/../b",
            "~/a/./b",
            "/a//b",
            "/a\nb",
        ] {
            let err =
                from_map(&[(HOME, "/Users/example"), (HIDE_HOST_HELPER_ROOT, bad)]).unwrap_err();
            assert_eq!(err[0].key, HIDE_HOST_HELPER_ROOT, "{bad}");
        }
        for good in ["~/.cache/hide-test/helper", "/tmp/hide-verify/helper"] {
            let env = from_map(&[(HOME, "/Users/example"), (HIDE_HOST_HELPER_ROOT, good)]).unwrap();
            assert_eq!(env.host_helper_root.as_deref(), Some(good));
            let env = from_map(&[(HOME, "/Users/example"), (HIDE_HOST_CLI_DIR, good)]).unwrap();
            assert_eq!(env.host_cli_dir.as_deref(), Some(good));
        }
        let err = from_map(&[(HOME, "/Users/example"), (HIDE_HOST_CLI_DIR, "bin")]).unwrap_err();
        assert_eq!(err[0].key, HIDE_HOST_CLI_DIR);
        assert_eq!(
            from_map(&[(HOME, "/Users/example")])
                .unwrap()
                .host_helper_root,
            None
        );
    }

    #[test]
    fn open_command_is_validated_at_boot() {
        let err = from_map(&[
            (HOME, "/Users/example"),
            (HIDE_OPEN_COMMAND, "relative-opener"),
        ])
        .unwrap_err();
        assert_eq!(err[0].key, HIDE_OPEN_COMMAND);
        let executable = std::env::current_exe().unwrap();
        let env = from_map(&[
            (HOME, "/Users/example"),
            (HIDE_OPEN_COMMAND, executable.to_str().unwrap()),
        ])
        .unwrap();
        assert_eq!(env.open_command.as_deref(), Some(executable.as_path()));
        // A file the system cannot start as a program: no execute bit on
        // Unix, no program extension on Windows.
        let directory = tempfile::tempdir().unwrap();
        let text = directory.path().join("opener.txt");
        std::fs::write(&text, "not a program").unwrap();
        let err = from_map(&[
            (HOME, "/Users/example"),
            (HIDE_OPEN_COMMAND, text.to_str().unwrap()),
        ])
        .unwrap_err();
        assert_eq!(err[0].key, HIDE_OPEN_COMMAND);
    }
}
