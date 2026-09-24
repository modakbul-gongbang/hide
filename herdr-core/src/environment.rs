use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::model::EnvironmentStatusSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvironmentVariableSpec {
    pub key: &'static str,
    pub required: bool,
    pub format: &'static str,
    pub absent_behavior: &'static str,
}

pub const PATH_KEY: &str = "PATH";
pub const HOME_KEY: &str = "HOME";
pub const SSH_AUTH_SOCK_KEY: &str = "SSH_AUTH_SOCK";
pub const HERDR_SOCKET_PATH_KEY: &str = "HERDR_SOCKET_PATH";
pub const CODEX_HOME_KEY: &str = "CODEX_HOME";

pub const REGISTRY: [EnvironmentVariableSpec; 5] = [
    EnvironmentVariableSpec {
        key: HOME_KEY,
        required: false,
        format: "absolute home-directory path",
        absent_behavior: "Provider usage and home-relative integrations are unavailable",
    },
    EnvironmentVariableSpec {
        key: SSH_AUTH_SOCK_KEY,
        required: false,
        format: "absolute Unix-domain socket path",
        absent_behavior: "A device whose ssh config names no IdentityFile or IdentityAgent cannot authenticate and says so on its row; every other device connects",
    },
    EnvironmentVariableSpec {
        key: PATH_KEY,
        required: false,
        format: "colon-separated executable search path containing the chromux install directory",
        absent_behavior: "Chromux actions are disabled with visible guidance",
    },
    EnvironmentVariableSpec {
        key: HERDR_SOCKET_PATH_KEY,
        required: false,
        format: "absolute Unix-domain socket path",
        absent_behavior: "Use the default local Herdr socket path",
    },
    EnvironmentVariableSpec {
        key: CODEX_HOME_KEY,
        required: false,
        format: "absolute Codex home-directory path",
        absent_behavior: "Use $HOME/.codex for Codex usage credentials and sessions",
    },
];

#[derive(Clone, Debug)]
pub struct EnvironmentReport {
    pub statuses: Vec<EnvironmentStatusSnapshot>,
    pub home_path: Option<PathBuf>,
    pub chromux_enabled: bool,
    pub herdr_socket_path_override: Option<String>,
    pub codex_home: Option<PathBuf>,
}

pub fn read_and_validate() -> EnvironmentReport {
    validate_with(|key| std::env::var_os(key))
}

fn validate_with(mut read: impl FnMut(&str) -> Option<OsString>) -> EnvironmentReport {
    let home = read(HOME_KEY);
    let chromux_path = home
        .as_ref()
        .map(PathBuf::from)
        .map(|home| home.join("Library/pnpm/chromux"));
    validate_with_chromux_path(&mut read, home, chromux_path.as_deref())
}

fn validate_with_chromux_path(
    mut read: impl FnMut(&str) -> Option<OsString>,
    home: Option<OsString>,
    chromux_path: Option<&Path>,
) -> EnvironmentReport {
    let mut statuses = Vec::with_capacity(REGISTRY.len());
    let mut chromux_enabled = true;
    let mut herdr_socket_path_override = None;
    let mut home_path = None;
    let mut codex_home = None;

    for spec in REGISTRY {
        let value = read(spec.key);
        let (state, message) = match spec.key {
            HOME_KEY => match home.as_ref() {
                None => (
                    "absent",
                    "Home directory is unavailable; provider usage and home-relative integrations are disabled",
                ),
                Some(value) if value.is_empty() || !Path::new(value).is_absolute() => (
                    "invalid",
                    "Home directory configuration is invalid; provider usage and home-relative integrations are disabled",
                ),
                Some(value) => {
                    home_path = Some(PathBuf::from(value));
                    ("available", "Home directory configuration is available")
                }
            },
            // Each device authenticates as its own ssh config says, so a
            // missing agent only stops the devices that would use it, and
            // those report it on their own rows.
            SSH_AUTH_SOCK_KEY => match value {
                None => (
                    "absent",
                    "SSH agent socket is unavailable; only devices whose ssh config names an IdentityFile or IdentityAgent can authenticate",
                ),
                Some(value) if value.is_empty() || !Path::new(&value).is_absolute() => (
                    "invalid",
                    "SSH agent socket configuration is invalid; only devices whose ssh config names an IdentityFile or IdentityAgent can authenticate",
                ),
                Some(_) => ("available", "SSH agent socket configuration is available"),
            },
            PATH_KEY => match value {
                None => {
                    chromux_enabled = false;
                    (
                        "absent",
                        "Executable search path is unavailable; chromux actions are disabled",
                    )
                }
                Some(value) if !path_exposes_chromux(&value, chromux_path) => {
                    chromux_enabled = false;
                    (
                        "invalid",
                        "Executable search path does not expose chromux; chromux actions are disabled",
                    )
                }
                Some(_) => ("available", "Executable search path exposes chromux"),
            },
            HERDR_SOCKET_PATH_KEY => match value {
                None => (
                    "default",
                    "Herdr socket override is absent; the default local socket is used",
                ),
                Some(value) if value.is_empty() || !Path::new(&value).is_absolute() => (
                    "invalid",
                    "Herdr socket override is invalid; the configured default remains in use",
                ),
                Some(value) => {
                    herdr_socket_path_override = Some(value.to_string_lossy().into_owned());
                    ("available", "Herdr socket override is available")
                }
            },
            CODEX_HOME_KEY => match value {
                None => {
                    codex_home = home_path.as_ref().map(|home: &PathBuf| home.join(".codex"));
                    (
                        "default",
                        "Codex home override is absent; $HOME/.codex is used",
                    )
                }
                Some(value) if value.is_empty() || !Path::new(&value).is_absolute() => (
                    "invalid",
                    "Codex home override is invalid; Codex usage credentials are unavailable",
                ),
                Some(value) => {
                    codex_home = Some(PathBuf::from(value));
                    ("available", "Codex home override is available")
                }
            },
            _ => unreachable!("every environment key is declared in REGISTRY"),
        };
        statuses.push(EnvironmentStatusSnapshot {
            key: spec.key.to_owned(),
            required: spec.required,
            format: spec.format.to_owned(),
            state: state.to_owned(),
            absent_behavior: spec.absent_behavior.to_owned(),
            message: message.to_owned(),
        });
    }

    EnvironmentReport {
        statuses,
        home_path,
        chromux_enabled,
        herdr_socket_path_override,
        codex_home,
    }
}

fn path_exposes_chromux(value: &OsString, chromux_path: Option<&Path>) -> bool {
    let Some(chromux_path) = chromux_path else {
        return false;
    };
    std::env::split_paths(value).any(|component| component.join("chromux") == chromux_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn registry_is_enumerable_and_does_not_expose_values() {
        assert_eq!(REGISTRY.len(), 5);
        assert_eq!(REGISTRY[0].key, "HOME");
        let secret_like_value = OsString::from("/private/tmp/private-agent.sock");
        let chromux_path = Path::new("/private/tmp/hide-environment-test/Library/pnpm/chromux");
        let report = validate_with_chromux_path(
            |key| match key {
                SSH_AUTH_SOCK_KEY | HERDR_SOCKET_PATH_KEY => Some(secret_like_value.clone()),
                PATH_KEY => Some(OsString::from(
                    "/private/tmp/hide-environment-test/Library/pnpm:/usr/bin",
                )),
                _ => None,
            },
            Some(OsString::from("/private/tmp/hide-environment-test")),
            Some(chromux_path),
        );
        let encoded = serde_json::to_string(&report.statuses).unwrap();
        assert!(report.chromux_enabled);
        assert!(!encoded.contains("private-agent.sock"));
        assert!(!encoded.contains("/private/tmp/hide-environment-test/Library/pnpm"));
    }

    #[test]
    fn an_absent_agent_socket_is_reported_without_disabling_devices() {
        let report = validate_with(|_| None);
        assert!(!report.chromux_enabled);
        assert_eq!(report.statuses[1].state, "absent");
        assert!(!report.statuses[1].required);
        assert!(report.statuses[1].message.contains("IdentityFile"));
        assert_eq!(report.statuses[3].state, "default");
        assert!(report.home_path.is_none());
        assert!(report.herdr_socket_path_override.is_none());
        assert!(report.codex_home.is_none());
    }

    #[test]
    fn every_registered_key_has_a_distinct_contract() {
        let contracts = REGISTRY
            .iter()
            .map(|spec| (spec.key, (spec.format, spec.absent_behavior)))
            .collect::<BTreeMap<_, _>>();
        assert_eq!(contracts.len(), REGISTRY.len());
        assert!(contracts.values().all(|(format, behavior)| {
            !format.trim().is_empty() && !behavior.trim().is_empty()
        }));
    }

    #[test]
    fn socket_override_and_path_capability_share_the_registry_boundary() {
        let report = validate_with(|key| match key {
            SSH_AUTH_SOCK_KEY => Some(OsString::from("/private/tmp/agent.sock")),
            PATH_KEY => Some(OsString::from("/usr/bin")),
            HERDR_SOCKET_PATH_KEY => Some(OsString::from("/private/tmp/herdr.sock")),
            _ => None,
        });

        assert!(!report.chromux_enabled);
        assert_eq!(
            report.herdr_socket_path_override.as_deref(),
            Some("/private/tmp/herdr.sock")
        );
        assert_eq!(report.statuses[2].state, "invalid");
        assert_eq!(report.statuses[3].state, "available");
    }

    #[test]
    fn invalid_provider_overrides_do_not_fall_back_to_home() {
        let report = validate_with(|key| match key {
            HOME_KEY => Some(OsString::from("/private/tmp/hide-home")),
            CODEX_HOME_KEY => Some(OsString::from("relative-codex")),
            _ => None,
        });

        assert_eq!(
            report.home_path,
            Some(PathBuf::from("/private/tmp/hide-home"))
        );
        assert!(report.codex_home.is_none());
        assert_eq!(report.statuses[4].state, "invalid");
    }
}
