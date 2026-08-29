//! Version and executable-selection rules for the bundled Herdr runtime.
//!
//! The app owns one minimum runtime contract: the exact version shipped in
//! the release. Agent CLIs are intentionally not version-gated here. Their
//! authentication and execution remain delegated to the selected CLI.

use std::cmp::Ordering;

pub const BUNDLED_HERDR_VERSION: &str = "0.8.2";
pub const BUNDLED_HERDR_MINIMUM_VERSION: &str = BUNDLED_HERDR_VERSION;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HerdrInstallation {
    pub path: String,
    pub version: Version,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HerdrSource {
    LiveSocket,
    Installed(HerdrInstallation),
    Bundled {
        path: String,
        version: Version,
        guidance: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Version {
    components: Vec<u64>,
}

impl Version {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let normalized = raw
            .trim()
            .strip_prefix('v')
            .unwrap_or(raw.trim())
            .split_once('+')
            .map(|(version, _)| version)
            .unwrap_or(raw.trim())
            .split_once('-')
            .map(|(version, _)| version)
            .unwrap_or(raw.trim());
        let components = normalized
            .split('.')
            .map(|component| {
                component
                    .parse::<u64>()
                    .map_err(|_| format!("invalid Herdr version: {raw}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if components.is_empty() {
            return Err(format!("invalid Herdr version: {raw}"));
        }
        Ok(Self { components })
    }

    pub fn as_string(&self) -> String {
        self.components
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(".")
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let length = self.components.len().max(other.components.len());
        (0..length)
            .map(|index| {
                self.components
                    .get(index)
                    .copied()
                    .unwrap_or(0)
                    .cmp(&other.components.get(index).copied().unwrap_or(0))
            })
            .find(|ordering| *ordering != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn select_source(
    socket_is_alive: bool,
    installed: Option<HerdrInstallation>,
    bundled_path: Option<String>,
) -> Result<HerdrSource, String> {
    if socket_is_alive {
        return Ok(HerdrSource::LiveSocket);
    }
    let minimum = Version::parse(BUNDLED_HERDR_MINIMUM_VERSION)?;
    if let Some(ref installed) = installed {
        if installed.version >= minimum {
            return Ok(HerdrSource::Installed(installed.clone()));
        }
    }
    let Some(path) = bundled_path else {
        return Err(
            "Herdr is unavailable: no live socket, compatible installation, or bundled runtime"
                .to_owned(),
        );
    };
    let guidance = installed.map(|installed| {
        format!(
            "Installed Herdr {} is below the bundled minimum {}; using the bundled runtime. Upgrade the installed CLI when convenient.",
            installed.version.as_string(),
            BUNDLED_HERDR_MINIMUM_VERSION
        )
    });
    Ok(HerdrSource::Bundled {
        path,
        version: minimum,
        guidance,
    })
}

/// Agent launch intentionally checks availability only. There is no lower
/// bound on a user's claude/codex CLI version, and no credential value enters
/// this module or the snapshot contract.
pub fn agent_cli_is_usable(executable_exists: bool) -> bool {
    executable_exists
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installation(version: &str) -> HerdrInstallation {
        HerdrInstallation {
            path: "/installed/herdr".to_owned(),
            version: Version::parse(version).expect("version"),
        }
    }

    #[test]
    fn live_socket_wins_over_every_executable_candidate() {
        assert_eq!(
            select_source(
                true,
                Some(installation("0.1.0")),
                Some("/bundle/herdr".to_owned())
            )
            .expect("source"),
            HerdrSource::LiveSocket
        );
    }

    #[test]
    fn compatible_installed_runtime_wins_when_socket_is_absent() {
        assert_eq!(
            select_source(
                false,
                Some(installation("0.8.2")),
                Some("/bundle/herdr".to_owned())
            )
            .expect("source"),
            HerdrSource::Installed(installation("0.8.2"))
        );
    }

    #[test]
    fn old_installation_falls_back_to_the_pinned_bundle_with_guidance() {
        let source = select_source(
            false,
            Some(installation("0.7.9")),
            Some("/bundle/herdr".to_owned()),
        )
        .expect("source");
        assert!(matches!(
            source,
            HerdrSource::Bundled {
                guidance: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn agent_cli_support_is_presence_based_without_a_version_gate() {
        assert!(agent_cli_is_usable(true));
        assert!(!agent_cli_is_usable(false));
    }
}
