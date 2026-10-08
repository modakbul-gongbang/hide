use hide_agent_adapter::{ADAPTERS, HookInstall};
use hide_agent_hooks::{AgentRuntime, guidance::GuidanceAgent};

#[test]
fn every_installed_hook_dialect_resolves_through_the_shared_adapter() {
    for row in ADAPTERS {
        for spelling in std::iter::once(row.id).chain(row.aliases.iter().copied()) {
            let spelling = spelling.to_ascii_uppercase();
            match row.hook {
                HookInstall::Runtime(dialect) => {
                    let runtime =
                        AgentRuntime::parse(&spelling).expect("installed runtime dialect");
                    assert_eq!(runtime.dialect(), dialect);
                    assert_eq!(runtime.id(), row.id);
                }
                HookInstall::Guidance(dialect) => {
                    let guidance =
                        GuidanceAgent::from_id(&spelling).expect("installed guidance dialect");
                    assert_eq!(guidance.id(), row.id);
                    let printed = hide_agent_hooks::guidance::stdout(guidance, "fixture context");
                    if dialect.prints_guidance() {
                        let actual: serde_json::Value =
                            serde_json::from_str(&printed.expect("session guidance")).unwrap();
                        let expected: serde_json::Value = serde_json::from_str(include_str!(
                            "../../../hide-agent-adapter/tests/fixtures/cursor-guidance.json"
                        ))
                        .unwrap();
                        assert_eq!(actual, expected);
                    } else {
                        assert_eq!(printed, None, "{}", row.id);
                    }
                    assert_eq!(guidance.dialect(), row.spawn_guard);
                    assert_eq!(guidance.dialect(), row.subagent_counts);
                    assert!(AgentRuntime::parse(&spelling).is_none());
                }
                HookInstall::None => {
                    assert!(AgentRuntime::parse(&spelling).is_none());
                    assert!(GuidanceAgent::from_id(&spelling).is_none());
                }
            }
        }
    }
}
