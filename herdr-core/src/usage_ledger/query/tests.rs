use super::*;
use crate::usage_ledger::store::Totals;

fn now(date: &str) -> u64 {
    format!("{date}T12:00:00Z")
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond() as u64
}

fn row(agent: &str, date: &str, input: u64) -> Daily {
    Daily {
        device: "own".into(),
        agent: agent.into(),
        day: date.into(),
        model: "model-a".into(),
        totals: Totals {
            calls: 1,
            tokens: Tokens {
                input,
                cache_read: 90_000_000,
                ..Tokens::default()
            },
            recorded_cost_micro_usd: 125_000,
            providers: vec!["openai".into()],
            ..Totals::default()
        },
    }
}

fn project(rows: &[Daily], progress: &[Progress], date: &str) -> Option<Projection> {
    prepare(
        rows,
        progress,
        None,
        "own",
        now(date),
        &jiff::tz::TimeZone::UTC,
    )
    .unwrap()
}

// D-03/D-08: nearest-rank quartiles over nonzero closed days, independent
// of today's unfinished spike and the vastly larger cache-read count.
#[test]
fn closed_day_quartiles_ignore_today_and_cache_and_keep_breakdowns() {
    let mut rows: Vec<_> = [1, 2, 3, 4, 5, 6, 7, 1000]
        .into_iter()
        .enumerate()
        .map(|(i, n)| row("pi", &format!("2026-10-{:02}", i + 1), n))
        .collect();
    rows.push(row("pi", "2026-10-09", 1_000_000));
    let view = project(&rows, &[], "2026-10-09").unwrap();
    let range = view.popover.all;
    let active: Vec<_> = range.days.iter().filter(|d| d.amount.calls > 0).collect();
    assert_eq!(
        active.iter().map(|d| d.level).collect::<Vec<_>>(),
        [1, 1, 2, 2, 3, 3, 4, 4, 4]
    );
    assert_eq!(range.amount.new_tokens, 1_001_028);
    assert_eq!(range.amount.tokens.cache_read, 810_000_000);
    assert_eq!(range.amount.cost.micro_usd, 1_125_000);
    assert_eq!((range.active_days, range.observed_days), (9, 9));
    assert_eq!((range.current_streak, range.longest_streak), (9, 9));
    assert_eq!(range.peak.unwrap().date, "2026-10-09");
    assert_eq!(range.days.iter().filter(|d| d.recent_week).count(), 7);
}

// D-09/D-12: a stable 45-day calendar while reading; completed short
// histories remove leading empty weeks and use max-ratio levels.
#[test]
fn first_backfill_is_pending_then_short_history_shrinks_once() {
    let rows = [row("pi", "2026-10-05", 25), row("pi", "2026-10-06", 100)];
    let progress = Progress {
        agent: Agent::Pi,
        reading: true,
        files_done: 412,
        files_total: 1280,
        limited: false,
        skipped: 0,
        failure: None,
    };
    let during = project(&rows, &[progress], "2026-10-07")
        .unwrap()
        .popover
        .all;
    assert_eq!(during.days.first().unwrap().date, "2026-08-23");
    assert_eq!((during.files_done, during.files_total), (412, 1280));
    assert!(during.reading && during.days.first().unwrap().pending);
    assert!(
        !during
            .days
            .iter()
            .find(|d| d.date == "2026-10-05")
            .unwrap()
            .pending
    );
    let after = project(&rows, &[], "2026-10-07").unwrap().popover.all;
    assert_eq!(after.days.first().unwrap().date, "2026-10-04");
    assert_eq!(after.days.len(), 4);
    assert_eq!(
        after.days.iter().map(|d| d.level).collect::<Vec<_>>(),
        [0, 1, 4, 0]
    );
    assert_eq!(after.current_streak, 2); // today may still become active
    assert_eq!(
        project(&rows, &[], "2026-10-08")
            .unwrap()
            .popover
            .all
            .current_streak,
        0
    );
    assert!(after.days.iter().all(|d| !d.pending));
    assert!(project(&[], &[], "2026-10-07").is_none());
}

// D-04/D-11: filtering changes totals and color only; shares stay present,
// and their hover cost covers retained history outside the visible grid.
#[test]
fn agent_filter_keeps_calendar_and_other_shares_with_full_history_cost() {
    let mut rows = vec![row("pi", "2026-01-01", 900), row("pi", "2026-10-01", 100)];
    for agent in ["omp", "grok", "opencode"] {
        rows.push(row(agent, "2026-10-01", 300));
    }
    let mut foreign = row("pi", "2026-10-01", 9_000_000);
    foreign.device = "remote".into();
    rows.push(foreign);
    let view = project(&rows, &[], "2026-10-02").unwrap();
    assert_eq!(view.popover.shares.len(), 4);
    assert_eq!(view.popover.all.amount.new_tokens, 1000);
    let pi = view
        .popover
        .filtered
        .iter()
        .find(|p| p.agent == "pi")
        .unwrap();
    assert_eq!(pi.range.amount.new_tokens, 100);
    assert_eq!(
        pi.range.days.first().unwrap().date,
        view.popover.all.days.first().unwrap().date
    );
    assert_eq!(view.popover.all.days.last().unwrap().week, 19);
    let pi = view
        .popover
        .shares
        .iter()
        .find(|p| p.contribution.agent == "pi")
        .unwrap();
    assert_eq!(pi.contribution.share_basis_points, 1000);
    assert_eq!(pi.since.as_deref(), Some("2026-01-01"));
    assert_eq!(pi.lifetime.cost.micro_usd, 250_000);
    assert_eq!(view.year.all.amount.new_tokens, 1900);
    let day = view
        .popover
        .all
        .days
        .iter()
        .find(|d| d.date == "2026-10-01")
        .unwrap();
    assert_eq!(day.top_agents.len(), 3);
    assert_eq!(day.other_agents, 1);
    assert_eq!(
        view.popover
            .filtered
            .iter()
            .find(|p| p.agent == "codex")
            .unwrap()
            .range
            .amount
            .calls,
        0
    );
}

// D-05/D-06 and the recorded Grok decision: missing prices never become a
// complete zero. Reasons are unique, bounded and preserved at day/range.
#[test]
fn partial_costs_waiting_and_incomplete_reasons_survive_aggregation() {
    let mut rows = Vec::new();
    for i in 0..10 {
        let mut value = row("pi", "2026-10-01", 100);
        value.model = format!("model-{i:02}");
        value.totals.recorded_cost_micro_usd = 0;
        value.totals.missing_cost_calls = 1;
        rows.push(value);
    }
    let mut incomplete = row("grok", "2026-10-01", 20);
    incomplete.totals.incomplete_calls = 1;
    incomplete.totals.missing_cost_calls = 1;
    incomplete.totals.recorded_cost_micro_usd = 0;
    rows.push(incomplete);
    let mut waiting = row("codex", "2026-10-01", 30);
    waiting.totals.missing_cost_calls = 1;
    waiting.totals.recorded_cost_micro_usd = 0;
    rows.push(waiting);
    rows.push(row("omp", "2026-10-01", 50));
    let view = project(&rows, &[], "2026-10-01").unwrap().year;
    let cost = &view.all.amount.cost;
    assert_eq!(cost.micro_usd, 125_000);
    assert!(cost.partial && cost.waiting && cost.incomplete);
    assert_eq!(cost.unpriced_models.len(), 8);
    assert_eq!(cost.other_unpriced_models, 3);
    assert_eq!(&view.all.days.last().unwrap().amount.cost, cost);
    assert_eq!(view.all.amount.new_tokens, 1100);
    assert!(
        view.filtered
            .iter()
            .find(|p| p.agent == "codex")
            .unwrap()
            .range
            .amount
            .cost
            .waiting
    );
}

// B17/B18: calendar days cross DST without a 24-hour subtraction; only the
// core node is shown, and 400-day retention does not expand the year grid.
#[test]
fn local_calendar_and_retention_boundaries_do_not_reassign_stored_days() {
    let rows = [
        row("pi", "2025-01-01", 9_000),
        row("pi", "2025-10-01", 100),
        row("pi", "2026-10-31", 20),
        row("pi", "2026-11-01", 30),
    ];
    let time = "2026-11-02T01:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond() as u64;
    let zone = jiff::tz::TimeZone::get("America/New_York").unwrap();
    let view = prepare(&rows, &[], None, "own", time, &zone)
        .unwrap()
        .unwrap();
    assert!(view.year.all.days.len() <= 371);
    assert_eq!(view.year.all.days.last().unwrap().date, "2026-11-01");
    assert_eq!(view.year.shares[0].lifetime.new_tokens, 150);
    assert_eq!(view.year.shares[0].since.as_deref(), Some("2025-10-01"));
    assert_eq!(view.year.all.amount.new_tokens, 50);
    assert_eq!(view.year.all.current_streak, 2);
}

#[test]
fn provider_capacity_refuses_instead_of_silently_omitting_provenance() {
    let mut rows: Vec<_> = (0..32)
        .map(|i| {
            let mut value = row("pi", "2026-10-01", 10);
            value.totals.providers = vec![format!("provider-{i}")];
            value
        })
        .collect();
    assert_eq!(
        project(&rows, &[], "2026-10-01").unwrap().year.shares[0]
            .providers
            .len(),
        32
    );
    let mut overflow = row("pi", "2026-10-01", 10);
    overflow.totals.providers = vec!["one-more-provider".into()];
    rows.push(overflow);
    assert_eq!(
        prepare(
            &rows,
            &[],
            None,
            "own",
            now("2026-10-01"),
            &jiff::tz::TimeZone::UTC
        ),
        Err("usage_provider_capacity")
    );
}
