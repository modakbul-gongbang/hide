use super::*;

fn instant(day: &str) -> u64 {
    format!("{day}T12:00:00Z")
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond() as u64
}

fn call(key: u8, day: &str) -> Event {
    Event {
        key: format!("{key:064x}"),
        at_ms: instant(day),
        model: "model-a".into(),
        tokens: Tokens {
            input: 100,
            output: 20,
            cache_read: 900,
            cache_write: 30,
            cache_write_1h: 10,
            reasoning: Some(5),
        },
        recorded_cost_micro_usd: Some(123),
        session: "native-session".into(),
        cwd: Some("/work/app".into()),
        provider: Some("native-provider".into()),
        incomplete: false,
    }
}

fn source() -> Candidate {
    Candidate {
        path: "/native/sessions/calls.jsonl".into(),
        size: 1200,
        modified_ms: instant("2026-10-01"),
    }
}

fn step(events: Vec<Event>) -> Step {
    Step {
        events,
        checkpoint: Checkpoint::default(),
        read_bytes: 1200,
        has_more: false,
        rescanned: false,
        skipped: Default::default(),
    }
}

#[test]
fn replay_copies_restart_and_changed_completed_values_never_add_twice() {
    let dir = tempfile::tempdir().unwrap();
    let path = hide_kit::layout::usage_store(dir.path());
    let zone = jiff::tz::TimeZone::UTC;
    let now = instant("2026-10-01");
    let first = step(vec![call(1, "2026-10-01"), call(1, "2026-10-01")]);
    {
        let mut store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .ingest("node-a", Agent::Claude, &source(), &first, now, &zone)
                .unwrap(),
            Ingested {
                added: 1,
                repeated: 1,
                ..Default::default()
            }
        );
    }
    let mut store = Store::open(&path).unwrap();
    let mut copied = source();
    copied.path = "/native/archive/copied.jsonl".into();
    assert_eq!(
        store
            .ingest("node-a", Agent::Claude, &copied, &first, now, &zone)
            .unwrap()
            .repeated,
        2
    );
    let mut changed = call(1, "2026-10-01");
    changed.tokens.input = 9999;
    assert_eq!(
        store
            .ingest(
                "node-a",
                Agent::Claude,
                &copied,
                &step(vec![changed]),
                now,
                &zone
            )
            .unwrap()
            .changed,
        1
    );
    let totals = &store.daily().unwrap()[0].totals;
    assert_eq!(
        (
            totals.calls,
            totals.new_tokens().unwrap(),
            totals.tokens.cache_read,
            totals.recorded_cost_micro_usd
        ),
        (1, 150, 900, 123)
    );
    assert!(
        store
            .checkpoint("node-a", Agent::Claude, source().path.to_str().unwrap())
            .unwrap()
            .is_some()
    );
}

#[test]
fn lost_checkpoints_after_sixty_days_cannot_change_frozen_or_recent_totals() {
    let dir = tempfile::tempdir().unwrap();
    let path = hide_kit::layout::usage_store(dir.path());
    let zone = jiff::tz::TimeZone::UTC;
    let mut store = Store::open(&path).unwrap();
    // First backfill may discover records older than 45 days.
    let original = step(vec![call(1, "2026-07-01"), call(2, "2026-09-01")]);
    store
        .ingest(
            "node-a",
            Agent::Claude,
            &source(),
            &original,
            instant("2026-09-01"),
            &zone,
        )
        .unwrap();
    store
        .complete_backfill("node-a", Agent::Claude, instant("2026-09-01"), &zone)
        .unwrap();
    let next = step(vec![call(3, "2026-10-01")]);
    store
        .ingest(
            "node-a",
            Agent::Claude,
            &source(),
            &next,
            instant("2026-10-31"),
            &zone,
        )
        .unwrap();
    store.prune(instant("2026-10-31"), &zone).unwrap();
    let expected = store.daily().unwrap();
    store.connection.execute("DELETE FROM cursors", []).unwrap();
    drop(store);
    let mut store = Store::open(&path).unwrap();
    let replay = step(vec![
        call(1, "2026-07-01"),
        call(2, "2026-09-01"),
        call(3, "2026-10-01"),
        call(4, "2026-08-01"),
    ]);
    let result = store
        .ingest(
            "node-a",
            Agent::Claude,
            &source(),
            &replay,
            instant("2026-10-31"),
            &zone,
        )
        .unwrap();
    assert_eq!((result.expired, result.repeated, result.added), (3, 1, 0));
    assert_eq!(store.daily().unwrap(), expected);
    assert!(store.backfill_complete("node-a", Agent::Claude).unwrap());
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM events", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn interrupted_backfill_keeps_old_call_identity_until_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = hide_kit::layout::usage_store(dir.path());
    let zone = jiff::tz::TimeZone::UTC;
    let first = step(vec![call(1, "2026-07-01")]);
    let now = instant("2026-10-01");
    let mut store = Store::open(&path).unwrap();
    store
        .ingest("node-a", Agent::Claude, &source(), &first, now, &zone)
        .unwrap();
    store.prune(now, &zone).unwrap();
    drop(store);
    let mut store = Store::open(&path).unwrap();
    assert_eq!(
        store
            .ingest("node-a", Agent::Claude, &source(), &first, now, &zone)
            .unwrap()
            .repeated,
        1
    );
    assert_eq!(store.daily().unwrap()[0].totals.calls, 1);
    store
        .complete_backfill("node-a", Agent::Claude, now, &zone)
        .unwrap();
    store.prune(instant("2027-08-05"), &zone).unwrap();
    assert!(store.daily().unwrap().is_empty());
    assert_eq!(
        store
            .ingest("node-a", Agent::Claude, &source(), &first, now, &zone)
            .unwrap()
            .expired,
        1,
        "clock rollback cannot reopen a frozen day"
    );
}

#[test]
fn batch_failure_rolls_back_totals_and_checkpoint_and_capacity_reports_a_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&hide_kit::layout::usage_store(dir.path())).unwrap();
    let zone = jiff::tz::TimeZone::UTC;
    let now = instant("2026-10-01");
    let mut invalid = call(2, "2026-10-01");
    invalid.key = "missing".into();
    assert_eq!(
        store.ingest(
            "node-a",
            Agent::Claude,
            &source(),
            &step(vec![call(1, "2026-10-01"), invalid]),
            now,
            &zone
        ),
        Err("usage_key_invalid")
    );
    assert!(store.daily().unwrap().is_empty());
    assert!(
        store
            .checkpoint("node-a", Agent::Claude, source().path.to_str().unwrap())
            .unwrap()
            .is_none()
    );
    store
        .connection
        .execute("INSERT INTO devices VALUES('node-a',?1)", [EVENT_LIMIT])
        .unwrap();
    let outcome = store
        .ingest(
            "node-a",
            Agent::Claude,
            &source(),
            &step(vec![call(1, "2026-10-01")]),
            now,
            &zone,
        )
        .unwrap();
    assert_eq!(outcome.limited, 1);
    assert!(store.daily().unwrap().is_empty());
}

#[test]
fn native_node_identity_and_original_local_day_survive_zone_changes_and_core_move() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let path = hide_kit::layout::usage_store(&state);
    let mut store = Store::open(&path).unwrap();
    let mut event = call(1, "2026-10-01");
    event.at_ms = "2026-10-01T23:30:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond() as u64;
    let now = instant("2026-10-02");
    let seoul = jiff::tz::TimeZone::get("Asia/Seoul").unwrap();
    store
        .ingest(
            "physical-node-a",
            Agent::Pi,
            &source(),
            &step(vec![event.clone()]),
            now,
            &seoul,
        )
        .unwrap();
    store
        .ingest(
            "physical-node-a",
            Agent::Pi,
            &source(),
            &step(vec![event]),
            now,
            &jiff::tz::TimeZone::UTC,
        )
        .unwrap();
    assert_eq!(store.daily().unwrap()[0].day, "2026-10-02");
    let ai = dir.path().join("ai.json");
    std::fs::write(&ai, r#"{"provider":"claude"}"#).unwrap();
    let destination = dir.path().join("copy");
    // Live WAL write must reach the moved copy through VACUUM INTO.
    let manifest = crate::node_migration::copy::stage(&state, &destination, &ai).unwrap();
    assert!(manifest.files.contains_key(hide_kit::layout::USAGE_STORE));
    let copy = Store::open(&hide_kit::layout::usage_store(&destination)).unwrap();
    assert_eq!(copy.daily().unwrap(), store.daily().unwrap());
}

#[test]
fn corrupt_or_newer_stores_are_refused_without_replacing_their_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = hide_kit::layout::usage_store(dir.path());
    std::fs::write(&path, b"damaged original").unwrap();
    assert!(matches!(Store::open(&path), Err("usage_store_corrupt")));
    assert_eq!(std::fs::read(&path).unwrap(), b"damaged original");
    std::fs::remove_file(&path).unwrap();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA user_version=99").unwrap();
    drop(db);
    assert!(matches!(Store::open(&path), Err("usage_store_newer")));
}

#[test]
fn expired_key_cannot_reenter_after_timezone_moves_its_day_across_the_horizon() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&hide_kit::layout::usage_store(dir.path())).unwrap();
    let west = jiff::tz::TimeZone::get("America/Los_Angeles").unwrap();
    let east = jiff::tz::TimeZone::get("Asia/Seoul").unwrap();
    let mut event = call(1, "2026-08-17");
    event.at_ms = "2026-08-17T06:00:00Z"
        .parse::<jiff::Timestamp>()
        .unwrap()
        .as_millisecond() as u64;
    let now = instant("2026-10-01");
    let batch = step(vec![event]);
    store
        .ingest("node-a", Agent::Claude, &source(), &batch, now, &west)
        .unwrap();
    store
        .complete_backfill("node-a", Agent::Claude, now, &west)
        .unwrap();
    let expected = store.daily().unwrap();
    let result = store
        .ingest("node-a", Agent::Claude, &source(), &batch, now, &east)
        .unwrap();
    assert_eq!(result.expired, 1);
    assert_eq!(store.daily().unwrap(), expected);
}
