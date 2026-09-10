//! Why a pane's children are visible, or why they are not.
//!
//! Two screens depend on this being one function rather than a guess made at
//! each surface: the tooltip on an uninstrumented mark, and the Settings
//! diagnosis. The order the reasons are tried in is fixed, and the first one
//! that matches is the one shown; nothing falls through to an empty value or
//! an invented cause (PRD B21, B32, D-64).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::install::{HookStatus, InstallFailure, status};
use crate::runtime::{AgentRuntime, HOOK_VERSION};

/// Why Hide cannot say what a pane's agent has spawned.
///
/// The variant order is the resolution order.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UninstrumentedReason {
    /// The runtime's configuration file could not be read or parsed, so Hide
    /// never installed anything into it.
    ConfigUnreadable,
    /// The pane belongs to a remote host. Hide does not write to another
    /// machine's file system (PRD D-28, D-49).
    RemoteHost,
    /// The runtime is here and carries no hook of Hide's.
    HooksNotInstalled,
    /// The hook is installed, but this session was already running when it
    /// was, so it never fired. Restarting the agent instruments it.
    SessionPredatesInstall,
    /// The session is reporting through an older hook than this Hide writes.
    HookOutdated,
    /// None of the above. The cause is genuinely unknown and is said to be,
    /// rather than guessed at or drawn as a zero.
    Unknown,
}

impl UninstrumentedReason {
    /// The stable name every surface keys on, so no screen has to compare
    /// the operator-facing sentence against a literal of its own.
    pub fn code(self) -> &'static str {
        match self {
            Self::ConfigUnreadable => "config_unreadable",
            Self::RemoteHost => "remote_host",
            Self::HooksNotInstalled => "hooks_not_installed",
            Self::SessionPredatesInstall => "session_predates_install",
            Self::HookOutdated => "hook_outdated",
            Self::Unknown => "unknown",
        }
    }

    /// Reads a code back, from the same table [`Self::code`] writes. A
    /// caller holding a projected reason can recover the resolution order
    /// without a second list of names.
    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::ConfigUnreadable,
            Self::RemoteHost,
            Self::HooksNotInstalled,
            Self::SessionPredatesInstall,
            Self::HookOutdated,
            Self::Unknown,
        ]
        .into_iter()
        .find(|reason| reason.code() == code)
    }

    /// The sentence the tooltip shows. It is the whole explanation: the
    /// operator must be able to tell "installed but this session is old" from
    /// "never installed" without opening anything.
    pub fn message(self) -> &'static str {
        match self {
            Self::ConfigUnreadable => {
                "Hide could not read this runtime's settings file, so its hook is not installed."
            }
            Self::RemoteHost => "Hide does not install hooks on remote hosts.",
            Self::HooksNotInstalled => "This runtime's Hide hook is not installed.",
            Self::SessionPredatesInstall => {
                "This session started before the Hide hook was installed. Restart the agent to instrument it."
            }
            Self::HookOutdated => "This session is reporting through an older Hide hook.",
            Self::Unknown => "Child information is unavailable for this pane.",
        }
    }

    /// The short mark's accessible name, so the symbol never carries the
    /// meaning alone (PRD B37).
    pub fn accessibility_label(self) -> &'static str {
        "Children unknown"
    }
}

/// What the core observed about one pane, in the vocabulary this judgement
/// needs. Everything here comes from the snapshot the core already has.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaneObservation {
    /// The pane belongs to a remote target.
    pub remote: bool,
    /// The runtime Herdr detected in this pane, when Hide has an adapter for
    /// it. `None` is an agent Hide cannot instrument.
    pub runtime: Option<AgentRuntime>,
    /// The version carried by the pane's `hide_hooks` token, when the hook
    /// reported at all.
    pub token_version: Option<u32>,
    pub working: Option<u32>,
    pub done: Option<u32>,
    pub blocked: Option<u32>,
}

/// What one pane's chip row is allowed to say.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaneInstrumentation {
    /// The counts below are true. False means the mark and its reason.
    pub instrumented: bool,
    pub reason: Option<UninstrumentedReason>,
    pub working: Option<u32>,
    pub done: Option<u32>,
    pub blocked: Option<u32>,
}

/// Resolves one pane against the runtime's install state.
///
/// `status` is the state of the pane's own runtime; a pane with no adapter
/// passes `None` and lands on the single fallback reason.
pub fn instrumentation(
    observation: PaneObservation,
    status: Option<&HookStatus>,
) -> PaneInstrumentation {
    let uninstrumented = |reason: UninstrumentedReason| PaneInstrumentation {
        instrumented: false,
        reason: Some(reason),
        working: None,
        done: None,
        blocked: None,
    };
    if observation.remote {
        return uninstrumented(UninstrumentedReason::RemoteHost);
    }
    let Some(status) = status else {
        return uninstrumented(UninstrumentedReason::Unknown);
    };
    match status {
        HookStatus::Failed { .. } => return uninstrumented(UninstrumentedReason::ConfigUnreadable),
        HookStatus::RuntimeAbsent | HookStatus::NotInstalled => {
            return uninstrumented(UninstrumentedReason::HooksNotInstalled);
        }
        HookStatus::Installed { .. } | HookStatus::Outdated { .. } => {}
    }
    match observation.token_version {
        None => uninstrumented(UninstrumentedReason::SessionPredatesInstall),
        Some(version) if version < HOOK_VERSION => {
            uninstrumented(UninstrumentedReason::HookOutdated)
        }
        Some(_) => PaneInstrumentation {
            instrumented: true,
            reason: None,
            working: observation.working,
            done: observation.done,
            blocked: observation.blocked,
        },
    }
}

/// One runtime's row in the Settings diagnosis and the CLI output.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RuntimeDiagnosis {
    pub runtime: AgentRuntime,
    pub label: String,
    pub path: String,
    pub status: HookStatus,
    /// The version this Hide would install.
    pub current_version: u32,
}

impl RuntimeDiagnosis {
    /// The short word beside the runtime's name.
    pub fn headline(&self) -> String {
        match &self.status {
            HookStatus::RuntimeAbsent => "Not on this Mac".to_owned(),
            HookStatus::Installed { version } => format!("Installed (v{version})"),
            HookStatus::Outdated { version } => {
                format!("Outdated (v{version}, current v{})", self.current_version)
            }
            HookStatus::NotInstalled => "Not installed".to_owned(),
            HookStatus::Failed { reason } => reason.message(),
        }
    }

    /// Whether the operator can be offered a reinstall for this runtime
    /// (PRD B28).
    pub fn offers_install(&self) -> bool {
        matches!(
            self.status,
            HookStatus::NotInstalled | HookStatus::Outdated { .. } | HookStatus::Failed { .. }
        )
    }

    /// Whether the operator can be offered a removal (PRD B29).
    ///
    /// A missing helper counts. Finding it is proof that Hide's own entries
    /// are in the file and readable - that is how the helper path was read in
    /// the first place - and those entries are exactly what the operator
    /// needs taken out when the binary they name is gone. The other failures
    /// do not count: Hide could not parse or read the file, so it has no idea
    /// what removing would touch.
    pub fn offers_removal(&self) -> bool {
        matches!(
            self.status,
            HookStatus::Installed { .. }
                | HookStatus::Outdated { .. }
                | HookStatus::Failed {
                    reason: InstallFailure::HelperMissing { .. }
                }
        )
    }
}

/// The whole hook-install picture, in one value both the app and the CLI
/// render, so the terminal answer cannot drift from the app's (PRD B30).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub runtimes: Vec<RuntimeDiagnosis>,
}

impl Diagnosis {
    /// Reads every runtime under `home`. It performs file reads and no
    /// writes, so it is safe to call from a background reader.
    pub fn read(home: &Path) -> Self {
        Self {
            runtimes: AgentRuntime::ALL
                .into_iter()
                .map(|runtime| RuntimeDiagnosis {
                    runtime,
                    label: runtime.label().to_owned(),
                    path: runtime.config_path(home).display().to_string(),
                    status: status(runtime, home),
                    current_version: HOOK_VERSION,
                })
                .collect(),
        }
    }

    pub fn status_of(&self, runtime: AgentRuntime) -> Option<&HookStatus> {
        self.runtimes
            .iter()
            .find(|row| row.runtime == runtime)
            .map(|row| &row.status)
    }

    /// Plain text, one line per runtime, for a terminal.
    pub fn render(&self) -> String {
        let width = self
            .runtimes
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0);
        self.runtimes
            .iter()
            .map(|row| {
                format!(
                    "{:width$}  {}\n{:width$}  {}",
                    row.label,
                    row.headline(),
                    "",
                    row.path,
                    width = width
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::InstallFailure;

    fn observation(token_version: Option<u32>) -> PaneObservation {
        PaneObservation {
            remote: false,
            runtime: Some(AgentRuntime::ClaudeCode),
            token_version,
            working: Some(2),
            done: Some(3),
            blocked: None,
        }
    }

    #[test]
    fn the_first_matching_reason_wins_in_the_documented_order() {
        let failed = HookStatus::Failed {
            reason: InstallFailure::Unparsable {
                path: "/Users/example/.claude/settings.json".to_owned(),
                detail: "expected value".to_owned(),
            },
        };
        // A remote pane is remote before anything else is considered.
        let mut remote = observation(Some(HOOK_VERSION));
        remote.remote = true;
        assert_eq!(
            instrumentation(remote, Some(&failed)).reason,
            Some(UninstrumentedReason::RemoteHost)
        );
        assert_eq!(
            instrumentation(observation(Some(HOOK_VERSION)), Some(&failed)).reason,
            Some(UninstrumentedReason::ConfigUnreadable)
        );
        assert_eq!(
            instrumentation(
                observation(Some(HOOK_VERSION)),
                Some(&HookStatus::NotInstalled)
            )
            .reason,
            Some(UninstrumentedReason::HooksNotInstalled)
        );
        assert_eq!(
            instrumentation(
                observation(None),
                Some(&HookStatus::Installed {
                    version: HOOK_VERSION
                })
            )
            .reason,
            Some(UninstrumentedReason::SessionPredatesInstall)
        );
        assert_eq!(
            instrumentation(
                observation(Some(HOOK_VERSION - 1)),
                Some(&HookStatus::Installed {
                    version: HOOK_VERSION
                })
            )
            .reason,
            Some(UninstrumentedReason::HookOutdated)
        );
    }

    #[test]
    fn an_agent_with_no_adapter_says_so_rather_than_guessing() {
        let mut unknown = observation(Some(HOOK_VERSION));
        unknown.runtime = None;
        let result = instrumentation(unknown, None);
        assert_eq!(result.reason, Some(UninstrumentedReason::Unknown));
        assert!(result.working.is_none(), "an unknown count is never a zero");
    }

    #[test]
    fn an_instrumented_session_reports_its_counts_and_no_reason() {
        let result = instrumentation(
            observation(Some(HOOK_VERSION)),
            Some(&HookStatus::Installed {
                version: HOOK_VERSION,
            }),
        );
        assert!(result.instrumented);
        assert_eq!(result.reason, None);
        assert_eq!((result.working, result.done), (Some(2), Some(3)));
        assert_eq!(
            result.blocked, None,
            "no adapter observes a blocked subagent"
        );
    }

    #[test]
    fn every_reason_carries_a_distinct_sentence() {
        let reasons = [
            UninstrumentedReason::ConfigUnreadable,
            UninstrumentedReason::RemoteHost,
            UninstrumentedReason::HooksNotInstalled,
            UninstrumentedReason::SessionPredatesInstall,
            UninstrumentedReason::HookOutdated,
            UninstrumentedReason::Unknown,
        ];
        let messages: std::collections::BTreeSet<_> =
            reasons.iter().map(|reason| reason.message()).collect();
        assert_eq!(messages.len(), reasons.len());
        assert!(reasons.iter().all(|reason| !reason.message().is_empty()));
    }

    #[test]
    fn the_diagnosis_names_each_runtime_its_file_and_what_it_offers() {
        let home = std::env::temp_dir().join(format!("hide-diagnosis-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let diagnosis = Diagnosis::read(&home);
        assert_eq!(diagnosis.runtimes.len(), 2);
        let claude = &diagnosis.runtimes[0];
        assert_eq!(claude.status, HookStatus::NotInstalled);
        assert!(claude.offers_install() && !claude.offers_removal());
        let codex = &diagnosis.runtimes[1];
        assert_eq!(codex.status, HookStatus::RuntimeAbsent);
        assert!(!codex.offers_install() && !codex.offers_removal());
        assert!(diagnosis.render().contains(".claude/settings.json"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
