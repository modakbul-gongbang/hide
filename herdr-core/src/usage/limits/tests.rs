use super::*;
use serde_json::{Value, json};
const CAPTURED_AT: u64 = 1_789_632_300;
#[test]
fn every_declared_source_has_samples_covering_its_declared_windows() {
    use hide_agent_adapter::usage::Support;
    for source in LimitSource::ALL {
        let samples: Vec<(String, u64)> = match source {
            LimitSource::ClaudeUsageText => {
                let frame: Value = serde_json::from_str(include_str!(
                    "../../../tests/fixtures/claude-usage/usage-2.1.274.json"
                ))
                .unwrap();
                vec![(frame["result"].as_str().unwrap().into(), CAPTURED_AT)]
            }
            LimitSource::CodexUsage => vec![
                (
                    include_str!("../../../tests/fixtures/usage/codex/five-hour-and-weekly.json")
                        .into(),
                    1_735_390_000,
                ),
                (
                    include_str!("../../../tests/fixtures/usage/codex/weekly-only.json").into(),
                    CAPTURED_AT,
                ),
            ],
            LimitSource::AntigravityUsage => vec![(
                include_str!("../../../tests/fixtures/usage/antigravity/usage-1.4.0.json").into(),
                1_792_100_000,
            )],
        };
        let readings: Vec<_> = samples
            .iter()
            .map(|(body, now)| parse(source, body, *now).unwrap())
            .collect();
        for adapter in hide_agent_adapter::ADAPTERS
            .iter()
            .filter(|a| a.usage.limit_sources().any(|s| s == source))
        {
            for kind in [
                WindowKind::FiveHour,
                WindowKind::Weekly,
                WindowKind::Monthly,
            ] {
                let declared = adapter.usage.window(kind) == Support::Supported(source);
                assert_eq!(
                    readings
                        .iter()
                        .any(|r| r.windows.iter().any(|w| w.kind == kind
                            && w.scope == Scope::All
                            && matches!(w.outcome, WindowOutcome::Open { .. }))),
                    declared,
                    "{} {kind:?}",
                    adapter.id
                );
                if declared && source.presence(kind) == Presence::PlanDependent {
                    assert!(
                        readings
                            .iter()
                            .any(|r| matches!(outcome(r, kind), WindowOutcome::NotOffered))
                    );
                }
            }
            assert_eq!(
                readings.iter().any(|r| r
                    .windows
                    .iter()
                    .any(|w| matches!(w.scope, Scope::Model { .. }))),
                adapter.usage.limits.scoped == Support::Supported(source)
            );
        }
    }
    let reading = parse(
        LimitSource::CodexUsage,
        include_str!("../../../tests/fixtures/usage/codex/five-hour-and-weekly.json"),
        1_735_390_000,
    )
    .unwrap();
    assert_eq!(
        outcome(&reading, WindowKind::FiveHour),
        &WindowOutcome::Open {
            used_percent: 15.0,
            resets_at_s: 1_735_401_600
        }
    );
    assert_eq!(
        outcome(&reading, WindowKind::Weekly),
        &WindowOutcome::Open {
            used_percent: 5.0,
            resets_at_s: 1_735_920_000
        }
    );
}
fn outcome(reading: &LimitReading, kind: WindowKind) -> &WindowOutcome {
    &reading
        .windows
        .iter()
        .find(|w| w.kind == kind && w.scope == Scope::All)
        .unwrap()
        .outcome
}
#[test]
fn captured_claude_session_and_weekly_lines_share_one_read() {
    let frame: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/claude-usage/usage-2.1.274.json"
    ))
    .unwrap();
    let reading = parse(
        LimitSource::ClaudeUsageText,
        frame["result"].as_str().unwrap(),
        CAPTURED_AT,
    )
    .unwrap();
    assert_eq!(
        outcome(&reading, WindowKind::FiveHour),
        &WindowOutcome::Open {
            used_percent: 4.0,
            resets_at_s: 1_789_646_400
        }
    );
    assert_eq!(
        outcome(&reading, WindowKind::Weekly),
        &WindowOutcome::Open {
            used_percent: 1.0,
            resets_at_s: 1_790_222_400
        }
    );
    assert_eq!(reading.windows.len(), 3);
    assert_eq!(
        reading.windows[2].scope,
        Scope::Model {
            label: "Fable".into()
        }
    );
}
#[test]
fn a_partial_format_change_drops_only_that_window() {
    let body = "Current session: changed format\nCurrent week (all models): 24% used · resets Sep 24 at 1pm (Asia/Seoul)";
    let read = parse(LimitSource::ClaudeUsageText, body, CAPTURED_AT).unwrap();
    assert_eq!(
        outcome(&read, WindowKind::FiveHour),
        &WindowOutcome::Unreadable {
            cause: Cause::FormatChanged
        }
    );
    assert!(matches!(
        outcome(&read, WindowKind::Weekly),
        WindowOutcome::Open {
            used_percent: 24.0,
            ..
        }
    ));
    assert_eq!(
        parse(
            LimitSource::ClaudeUsageText,
            "Current session: changed\nCurrent week (all models): changed",
            CAPTURED_AT
        ),
        Err(Cause::FormatChanged)
    );
}
#[test]
fn no_reset_distinguishes_not_started_from_unreadable() {
    for (percent, expected) in [
        (0, WindowOutcome::NotStarted),
        (
            1,
            WindowOutcome::Unreadable {
                cause: Cause::FormatChanged,
            },
        ),
    ] {
        let read = parse(
            LimitSource::ClaudeUsageText,
            &format!("Current session: {percent}% used\nCurrent week (all models): 24% used · resets Sep 24 at 1pm (Asia/Seoul)"),
            CAPTURED_AT,
        )
        .unwrap();
        assert_eq!(outcome(&read, WindowKind::FiveHour), &expected);
        assert!(matches!(
            outcome(&read, WindowKind::Weekly),
            WindowOutcome::Open { .. }
        ));
    }
}
#[test]
fn entirely_unreadable_windows_are_a_whole_read_failure() {
    for (source, body) in [
        (LimitSource::ClaudeUsageText, "Current session: 12% used\nCurrent week (all models): 24% used".to_owned()),
        (LimitSource::CodexUsage, json!({"rate_limit":{"primary_window":{"used_percent":12,"limit_window_seconds":18000,"reset_at":"changed"},"secondary_window":{"used_percent":24,"limit_window_seconds":604800,"reset_at":"changed"}}}).to_string()),
    ] {
        assert_eq!(parse(source, &body, CAPTURED_AT), Err(Cause::FormatChanged));
    }
}
#[test]
fn expired_session_does_not_erase_a_current_weekly_window() {
    let read = parse(LimitSource::ClaudeUsageText, "Current session: 90% used · resets Sep 17 at 9pm (Asia/Seoul)\nCurrent week (all models): 63% left · resets Sep 24 at 1:30pm (Asia/Seoul)", 1_789_646_500).unwrap();
    assert_eq!(
        outcome(&read, WindowKind::FiveHour),
        &WindowOutcome::Expired {
            reset_at_s: 1_789_646_400
        }
    );
    assert_eq!(
        outcome(&read, WindowKind::Weekly),
        &WindowOutcome::Open {
            used_percent: 37.0,
            resets_at_s: 1_790_224_200
        }
    );
}
#[test]
fn codex_plan_and_window_order_are_read_by_duration() {
    let weekly = json!({"used_percent":76,"limit_window_seconds":604800,"reset_at":1893456000});
    let five_hour = json!({"used_percent":11,"limit_window_seconds":18000,"reset_at":1893450000});
    for (primary, secondary) in [
        (weekly.clone(), five_hour.clone()),
        (five_hour, weekly.clone()),
    ] {
        let read = parse(
            LimitSource::CodexUsage,
            &json!({"rate_limit":{"primary_window":primary,"secondary_window":secondary}})
                .to_string(),
            CAPTURED_AT,
        )
        .unwrap();
        assert_eq!(
            outcome(&read, WindowKind::FiveHour),
            &WindowOutcome::Open {
                used_percent: 11.0,
                resets_at_s: 1893450000
            }
        );
        assert_eq!(
            outcome(&read, WindowKind::Weekly),
            &WindowOutcome::Open {
                used_percent: 76.0,
                resets_at_s: 1893456000
            }
        );
    }
    let read = parse(
        LimitSource::CodexUsage,
        &json!({"rate_limit":{"primary_window":weekly,"secondary_window":null}}).to_string(),
        CAPTURED_AT,
    )
    .unwrap();
    assert_eq!(
        outcome(&read, WindowKind::FiveHour),
        &WindowOutcome::NotOffered
    );
    assert_eq!(
        parse(LimitSource::CodexUsage, "{}", CAPTURED_AT),
        Err(Cause::FormatChanged)
    );
}
#[test]
fn scoped_parse_failure_keeps_other_windows_and_wall_time_year_rules() {
    let read = parse(LimitSource::ClaudeUsageText, "Current week (all models): 1% used · resets Dec 31, 2027 at 11pm (Asia/Seoul)\nCurrent week (Fable): 0% used · resets Sep 24 at 1pm (Mars/Olympus)", CAPTURED_AT).unwrap();
    assert_eq!(
        outcome(&read, WindowKind::Weekly),
        &WindowOutcome::Open {
            used_percent: 1.0,
            resets_at_s: 1_830_261_600
        }
    );
    assert_eq!(
        read.windows.last().unwrap().outcome,
        WindowOutcome::Unreadable {
            cause: Cause::FormatChanged
        }
    );
    let read = parse(
        LimitSource::ClaudeUsageText,
        "Current week (all models): 1% used · resets Jan 1 at 1am (Asia/Seoul)",
        CAPTURED_AT,
    )
    .unwrap();
    assert_eq!(
        outcome(&read, WindowKind::Weekly),
        &WindowOutcome::Open {
            used_percent: 1.0,
            resets_at_s: 1_798_732_800
        }
    );
}
#[test]
fn cost_fallback_is_signed_out_never_zero() {
    let frame: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/claude-usage/cost-2.1.274.json"
    ))
    .unwrap();
    assert_eq!(
        parse(
            LimitSource::ClaudeUsageText,
            frame["result"].as_str().unwrap(),
            CAPTURED_AT
        ),
        Err(Cause::SignedOut)
    );
}
#[test]
fn agys_gemini_bucket_is_the_weekly_window_and_other_groups_are_scoped() {
    let reading = parse(
        LimitSource::AntigravityUsage,
        include_str!("../../../tests/fixtures/usage/antigravity/usage-1.4.0.json"),
        1_792_100_000,
    )
    .unwrap();
    assert_eq!(
        outcome(&reading, WindowKind::Weekly),
        &WindowOutcome::Open {
            used_percent: 25.0,
            resets_at_s: 1_792_730_677
        }
    );
    assert_eq!(
        outcome(&reading, WindowKind::FiveHour),
        &WindowOutcome::NotOffered
    );
    let scoped = reading
        .windows
        .iter()
        .find(|w| w.scope != Scope::All)
        .unwrap();
    assert_eq!(
        (&scoped.scope, &scoped.outcome),
        (
            &Scope::Model {
                label: "Claude and GPT models".into()
            },
            &WindowOutcome::Open {
                used_percent: 0.0,
                resets_at_s: 1_792_736_514
            }
        )
    );
    assert_eq!(
        parse(
            LimitSource::AntigravityUsage,
            r#"{"command":{"data":{}}}"#,
            0
        ),
        Err(Cause::FormatChanged)
    );
}
