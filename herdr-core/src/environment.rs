use std::ffi::OsString;
use std::path::Path;

use crate::model::EnvironmentStatusSnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnvironmentVariableSpec {
    pub key: &'static str,
    pub required: bool,
    pub format: &'static str,
    pub absent_behavior: &'static str,
}

pub const REGISTRY: [EnvironmentVariableSpec; 1] = [EnvironmentVariableSpec {
    key: "SSH_AUTH_SOCK",
    required: false,
    format: "absolute Unix-domain socket path",
    absent_behavior: "Remote features are disabled; local features remain available",
}];

#[derive(Clone, Debug)]
pub struct EnvironmentReport {
    pub statuses: Vec<EnvironmentStatusSnapshot>,
    pub remote_enabled: bool,
}

pub fn read_and_validate() -> EnvironmentReport {
    validate_with(|key| std::env::var_os(key))
}

fn validate_with(mut read: impl FnMut(&str) -> Option<OsString>) -> EnvironmentReport {
    let mut statuses = Vec::with_capacity(REGISTRY.len());
    let mut remote_enabled = true;

    for spec in REGISTRY {
        let (state, message) = match read(spec.key) {
            None => {
                remote_enabled = false;
                (
                    "absent",
                    "SSH agent socket is unavailable; remote features are disabled",
                )
            }
            Some(value) if value.is_empty() || !Path::new(&value).is_absolute() => {
                remote_enabled = false;
                (
                    "invalid",
                    "SSH agent socket configuration is invalid; remote features are disabled",
                )
            }
            Some(_) => ("available", "SSH agent socket configuration is available"),
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
        remote_enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn registry_is_enumerable_and_does_not_expose_values() {
        assert_eq!(REGISTRY.len(), 1);
        assert_eq!(REGISTRY[0].key, "SSH_AUTH_SOCK");
        let secret_like_value = OsString::from("/private/tmp/private-agent.sock");
        let report = validate_with(|_| Some(secret_like_value.clone()));
        let encoded = serde_json::to_string(&report.statuses).unwrap();
        assert!(report.remote_enabled);
        assert!(!encoded.contains("private-agent.sock"));
    }

    #[test]
    fn absent_optional_socket_disables_only_remote_capability() {
        let report = validate_with(|_| None);
        assert!(!report.remote_enabled);
        assert_eq!(report.statuses[0].state, "absent");
        assert!(!report.statuses[0].required);
        assert!(report.statuses[0].message.contains("remote features"));
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
}
