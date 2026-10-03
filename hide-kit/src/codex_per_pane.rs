//! `Codex를 pane마다 실행`: Codex's shared daemon turned off, so a Codex the
//! operator starts by hand runs in its own pane and Herdr knows its session
//! (PRD overview-request-view D-21..D-26). Reading and switching the setting
//! is `hide_agent_hooks::codex_daemon`'s; this part only decides when.
//!
//! The setting is the operator's to give back: the part's switch turns the
//! daemon on again and the part then reads `Off`, which no pass reverses. A
//! daemon setting turned back on outside Hide reads `Off` the same way (B36).

use std::path::PathBuf;

use hide_agent_hooks::codex_daemon::{self, DaemonSetting};

use crate::{KitTarget, Observed};

/// What the reason line says while a shared daemon still runs: sessions
/// attached to it stay there, and only a Codex started now runs per pane
/// (D-23, B35).
pub(crate) const RUNNING_NOTE: &str = "새로 여는 Codex부터 적용";

fn codex_dir(target: &KitTarget) -> PathBuf {
    target.home.join(".codex")
}

pub(crate) fn location(target: &KitTarget) -> PathBuf {
    codex_dir(target).join("config.toml")
}

pub(crate) fn observe(target: &KitTarget) -> Observed {
    let Some(codex) = target.codex.as_deref() else {
        return Observed::Absent("Codex is not on this machine".to_owned());
    };
    match codex_daemon::read_setting(codex, &target.home, &target.stop) {
        Ok(setting) => match setting {
            DaemonSetting::Unsupported => Observed::Unsupported(format!(
                "this Codex has no {} setting, so it starts no shared daemon",
                codex_daemon::DAEMON_FEATURE
            )),
            // Capability and configuration are independent: a supported CLI
            // can be launched per pane even before this account sets it up.
            DaemonSetting::Off | DaemonSetting::On if !codex_dir(target).is_dir() => {
                Observed::SupportedAbsent("Codex is not set up on this machine".to_owned())
            }
            DaemonSetting::Off => Observed::Current,
            DaemonSetting::On => Observed::Missing,
        },
        Err(reason) => Observed::Blocked(reason),
    }
}

pub(crate) fn install(target: &KitTarget) -> Result<(), String> {
    let codex = target
        .codex
        .as_deref()
        .ok_or_else(|| "Codex is not on this machine".to_owned())?;
    codex_daemon::turn_off(codex, &target.home, &target.stop)
}

/// The operator turned the part off: Codex gets its shared daemon back.
pub(crate) fn turn_off(target: &KitTarget) -> Result<(), String> {
    let codex = target
        .codex
        .as_deref()
        .ok_or_else(|| "Codex is not on this machine".to_owned())?;
    codex_daemon::turn_on(codex, &target.home, &target.stop)
}

/// The reason line of the applied part: [`RUNNING_NOTE`] while a daemon
/// still answers, nothing once it is down. A daemon that cannot be asked is
/// not a reason to say anything (the part itself is in place).
pub(crate) fn note(target: &KitTarget) -> Option<String> {
    let codex = target.codex.as_deref()?;
    codex_daemon::daemon_running(codex, &target.home, &target.stop)
        .ok()
        .filter(|running| *running)
        .map(|_| RUNNING_NOTE.to_owned())
}
