//! Hide's entries in each agent runtime's hook file, through
//! `hide-agent-hooks`, which owns the file format and the rule that another
//! tool's entry is never rewritten (D-15).
//!
//! The entry runs `<kit_dir>/hide-agent-hooks` (`.exe` on Windows) and exits
//! quietly when that file is gone (D-11), so it points at the kit folder that
//! outlives a build: the app bundle on this Mac, the helper root's `current`
//! on a device.

use std::path::{Path, PathBuf};

use hide_agent_hooks::{AgentRuntime, HookStatus, InstallFailure, MemoryCompatibility};

use crate::{KitTarget, Observed, RemoveOutcome};

fn helper(target: &KitTarget) -> PathBuf {
    target.kit_dir.join(hide_agent_hooks::HELPER_BINARY_NAME)
}

pub(crate) fn observe(target: &KitTarget, runtime: AgentRuntime) -> Observed {
    let status = hide_agent_hooks::install::status(runtime, &target.home);
    let observed = match status {
        HookStatus::RuntimeAbsent => {
            return Observed::Absent(format!("{} is not set up on this machine", runtime.label()));
        }
        HookStatus::NotInstalled => Observed::Missing,
        HookStatus::Outdated { version } => Observed::Stale(format!(
            "version {version} of the hook is there; this build writes version {}",
            hide_agent_hooks::HOOK_VERSION
        )),
        HookStatus::Failed {
            reason: InstallFailure::HelperMissing { helper, .. },
        } => Observed::Stale(format!("the hook points at {helper}, which is gone")),
        HookStatus::Failed { reason } => return Observed::Blocked(reason.message()),
        HookStatus::Installed { .. } => {
            let wanted = helper(target);
            match hide_agent_hooks::installed_helper_path(runtime, &target.home) {
                Some(found) if Path::new(&found) == wanted => Observed::Current,
                Some(found) => Observed::Stale(format!(
                    "the hook runs {found}, not this build's {}",
                    wanted.display()
                )),
                None => Observed::Stale("the hook's command is not one Hide wrote".to_owned()),
            }
        }
    };
    if matches!(observed, Observed::Current) {
        return observed;
    }
    if !helper(target).is_file() {
        return Observed::Blocked(format!(
            "this build has no hook helper at {}",
            helper(target).display()
        ));
    }
    // Only a CLI that answers with a version older than the hook needs is a
    // reason not to install; one Hide cannot find may be on a PATH the
    // daemon does not see, and a hook for it does no harm.
    if let MemoryCompatibility::UpdateRequired {
        installed_version: Some(installed),
        minimum_version,
    } = hide_agent_hooks::runtime_compatibility(runtime, &target.home)
    {
        return Observed::Blocked(format!(
            "{} {installed} is older than {minimum_version}, which Hide's hook needs; update it",
            runtime.label()
        ));
    }
    observed
}

pub(crate) fn install(target: &KitTarget, runtime: AgentRuntime) -> Result<(), String> {
    hide_agent_hooks::install(runtime, &target.home, &helper(target))
        .map(|_| ())
        .map_err(|failure| failure.message())
}

pub(crate) fn remove(target: &KitTarget, runtime: AgentRuntime) -> RemoveOutcome {
    match hide_agent_hooks::remove(runtime, &target.home) {
        Ok(outcome) if outcome.removed_entries > 0 => RemoveOutcome::Removed,
        Ok(_) => RemoveOutcome::Absent,
        Err(failure) => RemoveOutcome::Failed {
            reason: failure.message(),
        },
    }
}
