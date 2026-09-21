//! Single registry for every environment key this crate reads.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvKey {
    pub key: &'static str,
    pub required: bool,
    pub format: &'static str,
    pub absent_behavior: &'static str,
}

pub const HERDR_SOCKET_PATH: &str = "HERDR_SOCKET_PATH";
pub const XDG_STATE_HOME: &str = "XDG_STATE_HOME";
pub const HIDE_STATE_DIR: &str = "HIDE_STATE_DIR";
pub const HIDE_KEEP_ALIVE: &str = "HIDE_KEEP_ALIVE";
pub const HIDE_VITE_ORIGIN: &str = "HIDE_VITE_ORIGIN";
pub const HIDE_PORT: &str = "HIDE_PORT";
pub const HIDE_IDLE_SECS: &str = "HIDE_IDLE_SECS";
pub const HOME: &str = "HOME";

pub const REGISTRY: &[EnvKey] = &[
    EnvKey {
        key: HERDR_SOCKET_PATH,
        required: false,
        format: "absolute Unix-domain socket path",
        absent_behavior: "Core starts without a Herdr socket and the sidebar shows that state",
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
        key: HOME,
        required: true,
        format: "absolute home-directory path",
        absent_behavior: "Boot fails; the state directory cannot be resolved",
    },
];

#[derive(Clone, Debug)]
pub struct Env {
    pub herdr_socket_path: Option<String>,
    pub state_dir: PathBuf,
    pub keep_alive: bool,
    pub vite_origin: Option<String>,
    pub bind: SocketAddr,
    pub idle_secs: u64,
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
        other => other,
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
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Env {
        herdr_socket_path,
        state_dir,
        keep_alive,
        vite_origin,
        bind: SocketAddr::from(([127, 0, 0, 1], port)),
        idle_secs,
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
}
