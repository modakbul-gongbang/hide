use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use hide_agent_adapter::{ADAPTERS, Capability, Feature, HookInstall, adapter};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

fn state<T>(capability: Capability<T>) -> &'static str {
    match capability {
        Capability::Available(_) => "available",
        Capability::Unavailable => "unavailable",
        Capability::Unconfirmed => "unconfirmed",
    }
}

#[test]
fn declarations_match_independent_support_samples_and_all_six_factory_facts() {
    let fixtures: Vec<Value> = serde_json::from_str(include_str!("fixtures/support.json")).unwrap();
    assert_eq!(
        ADAPTERS.len(),
        fixtures.len(),
        "every adapter needs a support fixture"
    );
    let mut names = BTreeSet::new();
    for row in ADAPTERS {
        let fixture = fixtures
            .iter()
            .find(|fixture| fixture["id"] == row.id)
            .unwrap_or_else(|| panic!("{}: missing support fixture", row.id));
        assert_eq!(
            row.key.adapter().id,
            row.id,
            "{}: AgentId points at another row",
            row.id
        );
        for name in std::iter::once(row.id).chain(row.aliases.iter().copied()) {
            assert!(
                names.insert(name.to_ascii_lowercase()),
                "{}: duplicate id/alias {name}",
                row.id
            );
            assert_eq!(
                adapter(&format!(" {} ", name.to_ascii_uppercase()))
                    .unwrap()
                    .id,
                row.id
            );
        }
        assert!(
            !row.bell || row.prompt_hook.is_some(),
            "{}: bell requires a verified prompt hook",
            row.id
        );
        for (kind, present) in [
            ("hook", !matches!(row.hook, HookInstall::None)),
            ("session", row.session.is_some()),
        ] {
            let sample = &fixture[kind];
            if present {
                let path = sample["sample"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{}: missing {kind} sample", row.id));
                assert!(
                    root().join(path).is_file(),
                    "{}: missing {kind} sample file {path}",
                    row.id
                );
            } else {
                assert!(
                    sample["absent"]
                        .as_str()
                        .is_some_and(|reason| !reason.is_empty()),
                    "{}: unsupported {kind} needs an explicit absence fixture",
                    row.id
                );
            }
        }
        let expected: Vec<Feature> = serde_json::from_value(fixture["features"].clone()).unwrap();
        let actual: Vec<_> = Feature::ALL
            .into_iter()
            .filter(|feature| row.supports(*feature))
            .collect();
        assert_eq!(
            actual, expected,
            "{}: established feature matrix changed",
            row.id
        );
        let factory = row.factory;
        for (name, actual) in [
            ("direct_ask", state(factory.direct_ask)),
            ("user_turn", state(factory.user_turn)),
            ("turn_end_and_answer", state(factory.turn_end_and_answer)),
            ("startup_guidance", state(factory.startup_guidance)),
            ("resume", state(factory.resume)),
            ("next_prompt_letters", state(factory.next_prompt_letters)),
        ] {
            assert_eq!(
                fixture["factory"][name], actual,
                "{}: missing or changed Factory fact {name}",
                row.id
            );
        }
    }
    assert!(adapter("future-agent").is_none());
}

#[test]
fn generated_web_contract_and_logo_document_sources_are_current() {
    let expected =
        serde_json::to_value(hide_agent_adapter::web_contract().collect::<Vec<_>>()).unwrap();
    let path = root().join("contracts/agent-adapters.json");
    let actual: Value = serde_json::from_slice(
        &std::fs::read(&path).expect("missing web contract: run the export_web_contract example"),
    )
    .unwrap();
    assert_eq!(
        actual, expected,
        "web contract drift: regenerate contracts/agent-adapters.json with export_web_contract"
    );
    let logos: Value = serde_json::from_slice(
        &std::fs::read(root().join("web/src/assets/agents/manifest.json")).unwrap(),
    )
    .unwrap();
    for row in ADAPTERS {
        for (name, url) in [("Docs", row.doc_url), ("Install", row.install_url)] {
            assert!(
                url.starts_with("https://") && !url.chars().any(char::is_whitespace),
                "{}: missing {name} URL",
                row.id
            );
        }
        let found = ["logos", "existing", "monogram"].into_iter().any(|group| {
            logos[group]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == row.logo_id)
        });
        assert!(
            found,
            "{}: missing logo/monogram manifest entry {}",
            row.id, row.logo_id
        );
    }
}
