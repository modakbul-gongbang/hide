//! Single registry for every environment key this crate reads.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

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
pub const HIDE_STATE_DIR: &str = "HIDE_STATE_DIR";
pub const HIDE_KEEP_ALIVE: &str = "HIDE_KEEP_ALIVE";
pub const HIDE_VITE_ORIGIN: &str = "HIDE_VITE_ORIGIN";
pub const HIDE_PORT: &str = "HIDE_PORT";
pub const HIDE_IDLE_SECS: &str = "HIDE_IDLE_SECS";
pub const HOME: &str = "HOME";
pub const HIDE_OPEN_COMMAND: &str = "HIDE_OPEN_COMMAND";

pub const REGISTRY: &[EnvKey] = &[
    EnvKey {
        key: HERDR_SOCKET_PATH,
        required: false,
        format: "absolute Unix-domain socket path",
        absent_behavior: "Core starts without a Herdr socket and the sidebar shows that state",
    },
    EnvKey {
        key: HERDR_BIN_PATH,
        required: false,
        format: "absolute path of the herdr binary; Herdr sets it in every pane it manages",
        absent_behavior: "The first `herdr` on PATH attaches pane terminals; with neither, no pane terminal can attach and the daemon logs it",
    },
    EnvKey {
        key: PATH,
        required: false,
        format: "colon-separated executable search path",
        absent_behavior: "Only HERDR_BIN_PATH can name the herdr binary",
    },
    EnvKey {
        key: XDG_STATE_HOME,
        required: false,
        format: "absolute directory path",
        absent_behavior: "State lives under $HOME/.local/state",
    },
    EnvKey {
        key: HIDE_STATE_DIR,
        required: false,
        format: "absolute directory path",
        absent_behavior: "State lives under $XDG_STATE_HOME/hide",
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
        format: "Unix-only absolute path of an executable CLI helper whose first argument is the file to open",
        absent_behavior: "The host OS handler opens it (macOS `open`, Windows ShellExecuteW association, Linux `xdg-open`)",
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
    pub keep_alive: bool,
    pub vite_origin: Option<String>,
    pub bind: SocketAddr,
    pub idle_secs: u64,
    /// Optional test/operator-selected host opener, validated before serving.
    pub open_command: Option<PathBuf>,
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

pub fn load() -> Result<Env, Vec<EnvError>> {
    load_from(|key| std::env::var(key).ok())
}

pub fn load_from(mut read: impl FnMut(&str) -> Option<String>) -> Result<Env, Vec<EnvError>> {
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
    let herdr_socket_path = match read(HERDR_SOCKET_PATH) {
        Some(value) if value.is_empty() => {
            errors.push(EnvError {
                key: HERDR_SOCKET_PATH,
                kind: "empty",
            });
            None
        }
        Some(value) => Some(value),
        None => {
            let default = home.join(".config/herdr/herdr.sock");
            default.exists().then(|| default.display().to_string())
        }
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
        None => read(PATH).and_then(|path| first_on_path(&path, "herdr")),
    };
    let state_dir = if let Some(dir) = read(HIDE_STATE_DIR) {
        if dir.is_empty() {
            errors.push(EnvError {
                key: HIDE_STATE_DIR,
                kind: "empty",
            });
            PathBuf::new()
        } else {
            PathBuf::from(dir)
        }
    } else {
        let xdg = read(XDG_STATE_HOME).filter(|value| !value.is_empty());
        xdg.map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
            .join("hide")
    };
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
        Some(value) if cfg!(windows) || !valid_program_path(Path::new(&value)) => {
            errors.push(EnvError {
                key: HIDE_OPEN_COMMAND,
                kind: "invalid",
            });
            None
        }
        Some(value) => Some(PathBuf::from(value)),
        None => None,
    };
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Env {
        home,
        herdr_socket_path,
        herdr_bin_path,
        state_dir,
        keep_alive,
        vite_origin,
        bind: SocketAddr::from(([127, 0, 0, 1], port)),
        idle_secs,
        open_command,
    })
}

fn valid_program_path(path: &Path) -> bool {
    if !path.is_absolute() || !path.is_file() {
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

fn first_on_path(path: &str, name: &str) -> Option<PathBuf> {
    path.split(':')
        .filter(|dir| !dir.is_empty())
        .map(|dir| Path::new(dir).join(name))
        .find(|candidate| candidate.is_file())
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

    #[test]
    fn missing_home_fails_boot() {
        let err = from_map(&[]).unwrap_err();
        assert_eq!(err[0].key, HOME);
        assert_eq!(err[0].kind, "missing");
    }

    #[test]
    fn defaults_state_dir_under_home() {
        let env = from_map(&[("HOME", "/Users/example")]).unwrap();
        assert_eq!(
            env.state_dir,
            PathBuf::from("/Users/example/.local/state/hide")
        );
        assert_eq!(env.idle_secs, 600);
        assert!(!env.keep_alive);
        assert!(env.herdr_socket_path.is_none());
        assert!(env.herdr_bin_path.is_none());
    }

    #[test]
    fn herdr_bin_path_wins_over_path_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let on_path = dir.path().join("herdr");
        std::fs::write(&on_path, b"").unwrap();
        let env = from_map(&[
            ("HOME", "/Users/example"),
            ("PATH", &format!("/nonexistent:{}", dir.path().display())),
        ])
        .unwrap();
        assert_eq!(env.herdr_bin_path.as_deref(), Some(on_path.as_path()));
        let env = from_map(&[
            ("HOME", "/Users/example"),
            ("PATH", &dir.path().display().to_string()),
            ("HERDR_BIN_PATH", "/opt/herdr/bin/herdr"),
        ])
        .unwrap();
        assert_eq!(
            env.herdr_bin_path.as_deref(),
            Some(Path::new("/opt/herdr/bin/herdr"))
        );
        let err = from_map(&[("HOME", "/Users/example"), ("HERDR_BIN_PATH", "")]).unwrap_err();
        assert_eq!(err[0].key, HERDR_BIN_PATH);
    }

    #[test]
    fn vite_origin_must_be_loopback() {
        let err = from_map(&[
            ("HOME", "/Users/example"),
            ("HIDE_VITE_ORIGIN", "http://example.com"),
        ])
        .unwrap_err();
        assert_eq!(err[0].key, HIDE_VITE_ORIGIN);
    }

    #[test]
    fn open_command_is_validated_at_boot() {
        let err = from_map(&[
            ("HOME", "/Users/example"),
            (HIDE_OPEN_COMMAND, "relative-opener"),
        ])
        .unwrap_err();
        assert_eq!(err[0].key, HIDE_OPEN_COMMAND);
        let executable = std::env::current_exe().unwrap();
        #[cfg(unix)]
        {
            let env = from_map(&[
                ("HOME", "/Users/example"),
                (HIDE_OPEN_COMMAND, executable.to_str().unwrap()),
            ])
            .unwrap();
            assert_eq!(env.open_command.as_deref(), Some(executable.as_path()));
        }
        #[cfg(windows)]
        {
            let err = from_map(&[
                ("HOME", "/Users/example"),
                (HIDE_OPEN_COMMAND, executable.to_str().unwrap()),
            ])
            .unwrap_err();
            assert_eq!(err[0].key, HIDE_OPEN_COMMAND);
        }
    }
}
