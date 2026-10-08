//! Reading what Hide's hook wrote onto a pane.
//!
//! The hook helper reports through `pane.report_metadata`, so its
//! values arrive as ordinary pane tokens on the session snapshot the core
//! already pulls. This module is the only place that reads them (PRD D-08,
//! D-33).
//!
//! Every value is optional and an unreadable one is dropped rather than
//! defaulted. A count Hide cannot read is unknown, and unknown is drawn as
//! unknown; it is never a zero, because a zero is a claim that the agent is
//! working alone (PRD B32, engineering rule 4).

use std::collections::BTreeMap;

use hide_agent_hooks::runtime::{
    AgentRuntime, BLOCKED_TOKEN, DONE_TOKEN, INSTRUMENTED_TOKEN, WORKING_TOKEN,
};
use serde_json::Value;

/// What one pane's tokens say about the session running in it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PaneHookTokens {
    /// The hook version this session reported with. Its presence is the proof
    /// that the session is instrumented at all, which is what separates "this
    /// agent is working alone" from "Hide cannot see this agent's children"
    /// (PRD B23, D-61).
    pub version: Option<u32>,
    pub working: Option<u32>,
    pub done: Option<u32>,
    pub blocked: Option<u32>,
}

impl PaneHookTokens {
    pub fn read(tokens: &BTreeMap<String, Value>) -> Self {
        Self {
            version: count(tokens, INSTRUMENTED_TOKEN),
            working: count(tokens, WORKING_TOKEN),
            done: count(tokens, DONE_TOKEN),
            blocked: count(tokens, BLOCKED_TOKEN),
        }
    }

    /// Whether this pane carries any of Hide's tokens at all.
    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// Herdr reports every token as a string, so a count is parsed from one. A
/// value that is not a count is dropped, never coerced to zero.
fn count(tokens: &BTreeMap<String, Value>, name: &str) -> Option<u32> {
    match tokens.get(name)? {
        Value::String(raw) => raw.trim().parse().ok(),
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        _ => None,
    }
}

/// The runtime behind an agent kind Herdr detected.
///
/// `None` is an agent Hide has no adapter for. It is the honest answer, and
/// it lands on the single fallback reason rather than being guessed into one
/// of the two Hide does know (PRD B32, D-64).
pub fn runtime_of(agent_kind: &str) -> Option<AgentRuntime> {
    AgentRuntime::from_id(agent_kind)
}

/// The hook that counts a pane's subagents: Claude Code's and Codex's
/// six-event hook, or the hook file of Grok and Cursor (PRD
/// grok-cursor-hooks). Its install state is what a pane's count is judged
/// against.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CountingHook {
    Runtime(AgentRuntime),
    Basic(hide_agent_hooks::guidance::GuidanceAgent),
}

/// The hook that counts subagents for an agent kind Herdr detected, `None`
/// for an agent whose subagents Hide cannot see.
pub fn counting_hook(agent_kind: &str) -> Option<CountingHook> {
    let row = hide_agent_adapter::adapter(agent_kind)?;
    row.subagent_counts?;
    match row.hook {
        hide_agent_adapter::HookInstall::Runtime(dialect) => {
            AgentRuntime::from_dialect(dialect).map(CountingHook::Runtime)
        }
        hide_agent_adapter::HookInstall::Guidance(_) => {
            hide_agent_hooks::guidance::GuidanceAgent::from_id(row.id).map(CountingHook::Basic)
        }
        hide_agent_adapter::HookInstall::None => None,
    }
}

/// The kit adapter whose switch governs `runtime`'s hook.
pub fn adapter_id(runtime: AgentRuntime) -> &'static str {
    runtime.id()
}

/// The prefix every pane id on a remote target carries.
///
/// It is the same rule the read-record ledger scopes itself by. A remote pane
/// is judged against its own device's kit, never this Mac's hook files
/// (`Runtime::derive_device_session`).
pub fn is_remote_pane(pane_id: &str) -> bool {
    pane_id.starts_with("remote:")
}

/// A device's hook for `runtime`, in the terms the pane judgement reads,
/// from what the device's kit last reported (PRD device-parity D-21). On a
/// device where Hide has certainly installed nothing (`declined`: never
/// allowed, or a platform it has no helper for) the hook is not installed;
/// a hook the kit could not put in place, for whatever reason its
/// row gives, is not installed either. A device whose kit Hide has not read,
/// including one whose first read failed, answers nothing, so the cause
/// stays unknown rather than guessed.
pub fn device_hook_status(
    declined: bool,
    kit: &crate::model::KitSnapshot,
    hook: CountingHook,
) -> Option<hide_agent_hooks::HookStatus> {
    if declined {
        return Some(hide_agent_hooks::HookStatus::NotInstalled);
    }
    kit_hook_status(kit, hook)
}

/// `hook`'s install state as a machine's kit last reported it: the hook part
/// of Claude Code and Codex, the hook piece of the agent's row for Grok and
/// Cursor. A kit that has not reported it answers nothing.
pub fn kit_hook_status(
    kit: &crate::model::KitSnapshot,
    hook: CountingHook,
) -> Option<hide_agent_hooks::HookStatus> {
    use hide_agent_hooks::HookStatus;
    use hide_kit::{ComponentId, ComponentState};
    let state = match hook {
        CountingHook::Runtime(runtime) => {
            let id = match runtime {
                AgentRuntime::ClaudeCode => ComponentId::ClaudeCodeHook,
                AgentRuntime::Codex => ComponentId::CodexHook,
            };
            kit.components.iter().find(|part| part.id == id)?.state
        }
        CountingHook::Basic(agent) => {
            kit.agents
                .iter()
                .find(|row| row.id == agent.id())?
                .hook
                .as_ref()?
                .state
        }
    };
    Some(match state {
        ComponentState::Installed => HookStatus::Installed {
            version: hide_agent_hooks::HOOK_VERSION,
        },
        ComponentState::Outdated => HookStatus::Outdated { version: 0 },
        ComponentState::Absent => HookStatus::RuntimeAbsent,
        // The agent's switch is off: the hook is absent on purpose, which is
        // not the same cause as one Hide never installed.
        ComponentState::Off => HookStatus::Off,
        ComponentState::NotInstalled | ComponentState::Removed | ComponentState::Failed => {
            HookStatus::NotInstalled
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Value::String((*value).to_owned())))
            .collect()
    }

    #[test]
    fn an_instrumented_pane_reports_its_version_and_the_counts_it_has() {
        let read = PaneHookTokens::read(&tokens(&[
            ("hide_hooks", "1"),
            ("hide_sub_working", "2"),
            ("hide_sub_done", "7"),
            ("summary", "unrelated"),
        ]));
        assert_eq!(read.version, Some(1));
        assert_eq!(read.working, Some(2));
        assert_eq!(read.done, Some(7));
        assert_eq!(
            read.blocked, None,
            "a count no adapter reports stays unknown"
        );
    }

    #[test]
    fn a_pane_with_none_of_hides_tokens_is_empty_rather_than_a_row_of_zeroes() {
        let read = PaneHookTokens::read(&tokens(&[("summary", "building"), ("elapsed", "3m")]));
        assert!(read.is_empty());
        assert_eq!((read.version, read.working, read.done), (None, None, None));
    }

    #[test]
    fn an_unreadable_count_is_dropped_and_never_coerced() {
        let read = PaneHookTokens::read(&tokens(&[
            ("hide_hooks", "1"),
            ("hide_sub_working", "many"),
            ("hide_sub_done", "-4"),
        ]));
        assert_eq!(read.version, Some(1));
        assert_eq!(read.working, None);
        assert_eq!(read.done, None);
    }

    #[test]
    fn a_hook_part_of_an_agent_that_is_off_reads_off_and_not_not_installed() {
        let kit = |state| crate::model::KitSnapshot {
            components: vec![crate::model::KitComponentSnapshot {
                id: hide_kit::ComponentId::ClaudeCodeHook,
                label: String::new(),
                state,
                reason: None,
                location: None,
            }],
            ..Default::default()
        };
        assert_eq!(
            device_hook_status(
                false,
                &kit(hide_kit::ComponentState::Off),
                CountingHook::Runtime(AgentRuntime::ClaudeCode)
            ),
            Some(hide_agent_hooks::HookStatus::Off)
        );
        assert_eq!(
            device_hook_status(
                false,
                &kit(hide_kit::ComponentState::NotInstalled),
                CountingHook::Runtime(AgentRuntime::ClaudeCode)
            ),
            Some(hide_agent_hooks::HookStatus::NotInstalled)
        );
    }

    #[test]
    fn each_runtime_names_the_kit_adapter_whose_switch_governs_its_hook() {
        for runtime in AgentRuntime::ALL {
            let adapter = hide_kit::agents::adapter(adapter_id(runtime)).unwrap();
            assert_eq!(
                adapter.hook,
                hide_kit::HookSupport::Part(match runtime {
                    AgentRuntime::ClaudeCode => hide_kit::ComponentId::ClaudeCodeHook,
                    AgentRuntime::Codex => hide_kit::ComponentId::CodexHook,
                })
            );
        }
    }

    #[test]
    fn only_the_two_shipped_adapters_resolve_and_anything_else_says_so() {
        assert_eq!(runtime_of("claude"), Some(AgentRuntime::ClaudeCode));
        assert_eq!(runtime_of("Codex"), Some(AgentRuntime::Codex));
        assert_eq!(runtime_of("unknown"), None);
        assert_eq!(runtime_of(""), None);
    }

    #[test]
    fn grok_and_cursor_count_through_their_own_hook_and_others_through_none() {
        use hide_agent_hooks::guidance::GuidanceAgent;
        assert_eq!(
            counting_hook("claude"),
            Some(CountingHook::Runtime(AgentRuntime::ClaudeCode))
        );
        assert_eq!(
            counting_hook("grok"),
            Some(CountingHook::Basic(GuidanceAgent::Grok))
        );
        assert_eq!(
            counting_hook("cursor"),
            Some(CountingHook::Basic(GuidanceAgent::Cursor))
        );
        assert_eq!(counting_hook("opencode"), None);
        assert_eq!(counting_hook("unknown"), None);
    }

    #[test]
    fn a_grok_pane_is_judged_by_the_hook_piece_of_its_agent_row() {
        use hide_agent_hooks::guidance::GuidanceAgent;
        let kit = |state| crate::model::KitSnapshot {
            agents: vec![crate::model::KitAgentSnapshot {
                id: "grok".to_owned(),
                label: "Grok".to_owned(),
                availability: hide_kit::Availability::Available,
                enabled: true,
                chosen: true,
                skill: crate::model::KitPieceSnapshot {
                    state: hide_kit::ComponentState::Installed,
                    reason: None,
                    location: None,
                },
                hook: Some(crate::model::KitPieceSnapshot {
                    state,
                    reason: None,
                    location: None,
                }),
                herdr: None,
                partial: true,
                features: Vec::new(),
                sessions: None,
                doc_url: String::new(),
            }],
            ..Default::default()
        };
        let grok = CountingHook::Basic(GuidanceAgent::Grok);
        assert_eq!(
            kit_hook_status(&kit(hide_kit::ComponentState::Installed), grok),
            Some(hide_agent_hooks::HookStatus::Installed {
                version: hide_agent_hooks::HOOK_VERSION
            })
        );
        assert_eq!(
            kit_hook_status(&kit(hide_kit::ComponentState::Off), grok),
            Some(hide_agent_hooks::HookStatus::Off)
        );
        assert_eq!(
            kit_hook_status(
                &kit(hide_kit::ComponentState::Installed),
                CountingHook::Basic(GuidanceAgent::Cursor)
            ),
            None,
            "a row the kit did not report leaves the cause unknown"
        );
    }

    #[test]
    fn a_remote_pane_is_recognised_by_the_same_prefix_the_read_ledger_uses() {
        assert!(is_remote_pane("remote:mini/w1:pA"));
        assert!(!is_remote_pane("w1:pA"));
    }
}
